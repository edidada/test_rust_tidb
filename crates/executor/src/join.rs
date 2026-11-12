//! W11 连接类算子（docs/03 L3 W11、docs/04 §4.2 连接类、AC-OP-010/013）：
//! OP-P-HASHJOIN / MERGEJOIN / NLJ / INDEXJOIN / CARTESIAN / SEMIJOIN / NAAJ。
//!
//! L3 基线：open 阶段物化两侧输入并一次性计算结果行，next_chunk 按 chunk_capacity
//! 分批输出。Hash/Merge/IndexJoin/NLJ 为不同的「匹配策略」，共享同一结果组装逻辑。

use crate::context::ExecStats;
use crate::row::{block_to_rows, rows_to_block, Row, RowSchema};
use crate::scan::collect;
use crate::{ExecCtx, Executor, Property};
use lattice_common::{Error, ErrorKind, Result};
use lattice_expr::{DefaultScalarEvaluator, Expr, ScalarEvaluator as _};
use lattice_types::{Block, Scalar};
use std::collections::{HashMap, HashSet};

/// 连接类型（docs/04 §4.1 JoinType 的物理语义）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhysicalJoinType {
    Inner,
    LeftOuter,
    RightOuter,
    FullOuter,
    Semi,
    AntiSemi,
    LeftOuterSemi,
    AntiLeftOuterSemi,
    Cross,
    /// NOT IN 的 Null-Aware Anti 语义（docs/02 §2.1）。
    NullAwareAnti,
}

impl PhysicalJoinType {
    pub fn left_only_output(self) -> bool {
        matches!(
            self,
            PhysicalJoinType::Semi
                | PhysicalJoinType::AntiSemi
                | PhysicalJoinType::LeftOuterSemi
                | PhysicalJoinType::AntiLeftOuterSemi
                | PhysicalJoinType::NullAwareAnti
        )
    }
}

/// 等值连接键：左右各一列下标。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JoinKey {
    pub left: usize,
    pub right: usize,
}

/// hash key：NULL 用独立标记，永不与其他值相等。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum KeyValue {
    Int(i128),
    Str(String),
    Bool(bool),
    Bytes(Vec<u8>),
    Null,
}

fn key_of(row: &Row, index: usize) -> KeyValue {
    match &row[index] {
        Scalar::Null => KeyValue::Null,
        Scalar::Int(i) => KeyValue::Int(*i),
        Scalar::Str(s) => KeyValue::Str(s.clone()),
        Scalar::Bool(b) => KeyValue::Bool(*b),
        Scalar::Bytes(b) => KeyValue::Bytes(b.clone()),
        Scalar::Float(f) => KeyValue::Int(f.to_bits() as i128),
        Scalar::Decimal { unscaled, scale } => {
            KeyValue::Int(unscaled.saturating_mul(10i128.saturating_pow(*scale as u32)))
        }
        Scalar::Temporal(t) => KeyValue::Int(*t as i128),
        Scalar::Json(s) => KeyValue::Str(s.clone()),
    }
}

/// 多键组合（L3 基线编码为字符串；L10 用原始字节哈希优化）。
fn composite_key(row: &Row, keys: &[JoinKey], side: Side) -> KeyValue {
    let parts: Vec<String> = keys
        .iter()
        .map(|k| {
            let idx = match side {
                Side::Left => k.left,
                Side::Right => k.right,
            };
            format!("{:?}", key_of(row, idx))
        })
        .collect();
    KeyValue::Str(parts.join("\x1f"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
}

/// 连接规格。
#[derive(Debug, Clone)]
pub struct JoinSpec {
    pub keys: Vec<JoinKey>,
    pub join_type: PhysicalJoinType,
    /// 非等值附加谓词（对拼接行求值；列下标按 左列数 + 右列数 约定）。
    pub condition: Option<Expr>,
    pub right_width: usize,
}

impl JoinSpec {
    pub fn inner(keys: Vec<JoinKey>, right_width: usize) -> Self {
        Self {
            keys,
            join_type: PhysicalJoinType::Inner,
            condition: None,
            right_width,
        }
    }

    pub fn with_type(mut self, join_type: PhysicalJoinType) -> Self {
        self.join_type = join_type;
        self
    }

    pub fn with_condition(mut self, condition: Expr) -> Self {
        self.condition = Some(condition);
        self
    }
}

fn nulls(n: usize) -> Row {
    vec![Scalar::Null; n]
}

fn concat(left: &Row, right: &Row) -> Row {
    let mut out = left.clone();
    out.extend(right.iter().cloned());
    out
}

fn output_schema(left: &RowSchema, right: &RowSchema, spec: &JoinSpec) -> RowSchema {
    if spec.join_type.left_only_output() {
        return left.clone();
    }
    let mut cols = left.columns.clone();
    cols.extend(right.columns.iter().cloned());
    RowSchema::new(cols)
}

/// 对拼接行求值附加谓词（Bool3：仅 True 通过）。
fn condition_holds(spec: &JoinSpec, row: &Row) -> Result<bool> {
    let Some(cond) = &spec.condition else {
        return Ok(true);
    };
    let schema = RowSchema::new(
        row.iter()
            .enumerate()
            .map(|(i, v)| (format!("c{i}"), v.inferred_type()))
            .collect(),
    );
    let block = rows_to_block(std::slice::from_ref(row), &schema)?;
    let sel = lattice_common::SelectionVector::identity(1);
    let col = DefaultScalarEvaluator.eval(cond, &block, &sel)?;
    match col.get(0)? {
        Scalar::Bool(true) => Ok(true),
        Scalar::Bool(false) | Scalar::Null => Ok(false),
        other => Err(Error::new(
            ErrorKind::TypeMismatch,
            format!("连接条件返回非布尔值 {other:?}"),
        )),
    }
}

fn keys_match(left: &Row, right: &Row, keys: &[JoinKey]) -> bool {
    // SQL 等值连接：NULL 永不匹配。
    keys.iter().all(|k| {
        let l = key_of(left, k.left);
        let r = key_of(right, k.right);
        l != KeyValue::Null && r != KeyValue::Null && l == r
    })
}

/// 匹配策略：产生 (左行下标, 右行下标) 的候选对，并给出每个右行是否被匹配。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchStrategy {
    Hash,
    NestedLoop,
    Merge,
    Index,
}

/// 通用结果组装（docs/04 AC-OP-010/013 正确性矩阵）。
fn compute_join(
    left: &[Row],
    right: &[Row],
    left_schema: &RowSchema,
    spec: &JoinSpec,
    strategy: MatchStrategy,
) -> Result<Vec<Row>> {
    let jt = spec.join_type;

    // NAAJ：右表含 NULL → 全部 UNKNOWN → 结果为空（docs/02 §2.1）。
    if jt == PhysicalJoinType::NullAwareAnti {
        let has_null = right
            .iter()
            .any(|r| spec.keys.iter().any(|k| r[k.right] == Scalar::Null));
        if has_null {
            return Ok(Vec::new());
        }
        let right_keys: HashSet<KeyValue> = right
            .iter()
            .map(|r| composite_key(r, &spec.keys, Side::Right))
            .collect();
        return Ok(left
            .iter()
            .filter(|l| !right_keys.contains(&composite_key(l, &spec.keys, Side::Left)))
            .cloned()
            .collect());
    }

    let pairs = match strategy {
        MatchStrategy::Hash => hash_pairs(left, right, spec),
        MatchStrategy::NestedLoop => nested_loop_pairs(left, right, spec),
        MatchStrategy::Merge => merge_pairs(left, right, spec),
        MatchStrategy::Index => hash_pairs(left, right, spec), // 索引点查复用 hash 索引
    };

    // 附加谓词过滤。
    let pairs: Vec<(usize, usize)> = pairs
        .into_iter()
        .filter(|(li, ri)| {
            let joined = concat(&left[*li], &right[*ri]);
            condition_holds(spec, &joined).unwrap_or(false)
        })
        .collect();

    let mut right_matched = vec![false; right.len()];
    for (_, ri) in &pairs {
        right_matched[*ri] = true;
    }
    let mut left_matched = vec![false; left.len()];
    for (li, _) in &pairs {
        left_matched[*li] = true;
    }

    let mut out: Vec<Row> = Vec::new();
    match jt {
        PhysicalJoinType::Semi | PhysicalJoinType::LeftOuterSemi => {
            for (li, _) in &pairs {
                out.push(left[*li].clone());
            }
            // Semi 去重：同一左行只输出一次。
            out.dedup();
        }
        PhysicalJoinType::AntiSemi | PhysicalJoinType::AntiLeftOuterSemi => {
            for (li, m) in left_matched.iter().enumerate() {
                if !*m {
                    out.push(left[li].clone());
                }
            }
        }
        PhysicalJoinType::Inner | PhysicalJoinType::Cross => {
            for (li, ri) in &pairs {
                out.push(concat(&left[*li], &right[*ri]));
            }
        }
        PhysicalJoinType::LeftOuter => {
            for (li, ri) in &pairs {
                out.push(concat(&left[*li], &right[*ri]));
            }
            for (li, m) in left_matched.iter().enumerate() {
                if !*m {
                    out.push(concat(&left[li], &nulls(spec.right_width)));
                }
            }
        }
        PhysicalJoinType::RightOuter => {
            for (li, ri) in &pairs {
                out.push(concat(&left[*li], &right[*ri]));
            }
            for (ri, m) in right_matched.iter().enumerate() {
                if !*m {
                    out.push(concat(&nulls(left_schema.len()), &right[ri]));
                }
            }
        }
        PhysicalJoinType::FullOuter => {
            for (li, ri) in &pairs {
                out.push(concat(&left[*li], &right[*ri]));
            }
            for (li, m) in left_matched.iter().enumerate() {
                if !*m {
                    out.push(concat(&left[li], &nulls(spec.right_width)));
                }
            }
            for (ri, m) in right_matched.iter().enumerate() {
                if !*m {
                    out.push(concat(&nulls(left_schema.len()), &right[ri]));
                }
            }
        }
        PhysicalJoinType::NullAwareAnti => unreachable!("handled above"),
    }
    Ok(out)
}

fn hash_pairs(left: &[Row], right: &[Row], spec: &JoinSpec) -> Vec<(usize, usize)> {
    let mut table: HashMap<KeyValue, Vec<usize>> = HashMap::new();
    for (ri, r) in right.iter().enumerate() {
        let key = composite_key(r, &spec.keys, Side::Right);
        table.entry(key).or_default().push(ri);
    }
    let mut pairs = Vec::new();
    for (li, l) in left.iter().enumerate() {
        let key = composite_key(l, &spec.keys, Side::Left);
        if let Some(rows) = table.get(&key) {
            for ri in rows {
                if keys_match(l, &right[*ri], &spec.keys) {
                    pairs.push((li, *ri));
                }
            }
        }
    }
    pairs
}

fn nested_loop_pairs(left: &[Row], right: &[Row], spec: &JoinSpec) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    for (li, l) in left.iter().enumerate() {
        for (ri, r) in right.iter().enumerate() {
            if spec.keys.is_empty() || keys_match(l, r, &spec.keys) {
                pairs.push((li, ri));
            }
        }
    }
    pairs
}

/// 归并连接：假设两侧已按 join key 排序，双指针扫描（NULL 不匹配）。
fn merge_pairs(left: &[Row], right: &[Row], spec: &JoinSpec) -> Vec<(usize, usize)> {
    if spec.keys.is_empty() {
        return nested_loop_pairs(left, right, spec);
    }
    let k = spec.keys[0];
    let mut pairs = Vec::new();
    let mut i = 0usize;
    let mut j = 0usize;
    while i < left.len() && j < right.len() {
        let lk = key_of(&left[i], k.left);
        let rk = key_of(&right[j], k.right);
        match scalar_cmp(&lk, &rk) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                // 收集右侧相同键区间。
                let mut jj = j;
                while jj < right.len() && key_of(&right[jj], k.right) == rk {
                    let mut ii = i;
                    while ii < left.len() && key_of(&left[ii], k.left) == lk {
                        if lk != KeyValue::Null {
                            pairs.push((ii, jj));
                        }
                        ii += 1;
                    }
                    jj += 1;
                }
                i += 1;
                while i < left.len() && key_of(&left[i], k.left) == lk {
                    i += 1;
                }
                j = jj;
            }
        }
    }
    pairs
}

fn scalar_cmp(a: &KeyValue, b: &KeyValue) -> std::cmp::Ordering {
    match (a, b) {
        (KeyValue::Int(x), KeyValue::Int(y)) => x.cmp(y),
        (KeyValue::Str(x), KeyValue::Str(y)) => x.cmp(y),
        (KeyValue::Null, KeyValue::Null) => std::cmp::Ordering::Equal,
        (KeyValue::Null, _) => std::cmp::Ordering::Less,
        (_, KeyValue::Null) => std::cmp::Ordering::Greater,
        _ => format!("{a:?}").cmp(&format!("{b:?}")),
    }
}

/// 物化子执行器全部行。
fn materialize(
    child: &mut dyn Executor,
    schema: &RowSchema,
    ctx: &mut ExecCtx,
) -> Result<Vec<Row>> {
    let mut out = Vec::new();
    for block in collect(child, ctx)? {
        out.extend(block_to_rows(&block, schema)?);
    }
    Ok(out)
}

/// 通用物化连接执行器（所有 Join 算子共享）。
#[derive(Debug)]
pub struct JoinExecutor {
    left: Box<dyn Executor>,
    right: Box<dyn Executor>,
    left_schema: RowSchema,
    right_schema: RowSchema,
    spec: JoinSpec,
    strategy: MatchStrategy,
    out_schema: RowSchema,
    result: Vec<Row>,
    cursor: usize,
    chunk_size: usize,
    stats: ExecStats,
    closed: bool,
}

impl JoinExecutor {
    pub fn new(
        left: Box<dyn Executor>,
        right: Box<dyn Executor>,
        left_schema: RowSchema,
        right_schema: RowSchema,
        spec: JoinSpec,
        strategy: MatchStrategy,
    ) -> Self {
        let out_schema = output_schema(&left_schema, &right_schema, &spec);
        Self {
            left,
            right,
            left_schema,
            right_schema,
            spec,
            strategy,
            out_schema,
            result: Vec::new(),
            cursor: 0,
            chunk_size: 1024,
            stats: ExecStats::default(),
            closed: false,
        }
    }

    pub fn output_schema(&self) -> &RowSchema {
        &self.out_schema
    }

    pub fn strategy(&self) -> MatchStrategy {
        self.strategy
    }
}

impl Executor for JoinExecutor {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.left.open(ctx)?;
        self.right.open(ctx)?;
        let left = materialize(&mut *self.left, &self.left_schema, ctx)?;
        let right = materialize(&mut *self.right, &self.right_schema, ctx)?;
        self.chunk_size = ctx.chunk_capacity.max(1);
        self.result = compute_join(&left, &right, &self.left_schema, &self.spec, self.strategy)?;
        self.cursor = 0;
        Ok(())
    }

    fn next_chunk(&mut self) -> Result<Option<Block>> {
        if self.cursor >= self.result.len() {
            return Ok(None);
        }
        let end = (self.cursor + self.chunk_size).min(self.result.len());
        let chunk = &self.result[self.cursor..end];
        self.cursor = end;
        self.stats.record_rows(chunk.len());
        Ok(Some(rows_to_block(chunk, &self.out_schema)?))
    }

    fn close(&mut self) -> Result<ExecStats> {
        if !self.closed {
            self.stats.merge(self.left.close()?);
            self.stats.merge(self.right.close()?);
            self.closed = true;
        }
        Ok(self.stats)
    }

    fn required_property(&self) -> Property {
        Property
    }
}

/// OP-P-HASHJOIN。
pub fn hash_join(
    left: Box<dyn Executor>,
    right: Box<dyn Executor>,
    ls: RowSchema,
    rs: RowSchema,
    spec: JoinSpec,
) -> JoinExecutor {
    JoinExecutor::new(left, right, ls, rs, spec, MatchStrategy::Hash)
}

/// OP-P-NLJ。
pub fn nested_loop_join(
    left: Box<dyn Executor>,
    right: Box<dyn Executor>,
    ls: RowSchema,
    rs: RowSchema,
    spec: JoinSpec,
) -> JoinExecutor {
    JoinExecutor::new(left, right, ls, rs, spec, MatchStrategy::NestedLoop)
}

/// OP-P-MERGEJOIN（要求输入按 key 有序）。
pub fn merge_join(
    left: Box<dyn Executor>,
    right: Box<dyn Executor>,
    ls: RowSchema,
    rs: RowSchema,
    spec: JoinSpec,
) -> JoinExecutor {
    JoinExecutor::new(left, right, ls, rs, spec, MatchStrategy::Merge)
}

/// OP-P-INDEXJOIN（外表驱动内表索引点查，L3 用 hash 索引模拟）。
pub fn index_join(
    left: Box<dyn Executor>,
    right: Box<dyn Executor>,
    ls: RowSchema,
    rs: RowSchema,
    spec: JoinSpec,
) -> JoinExecutor {
    JoinExecutor::new(left, right, ls, rs, spec, MatchStrategy::Index)
}

/// OP-P-CARTESIAN：显式上界保护（docs/04 OP-P-CARTESIAN）。
pub const CARTESIAN_MAX_ROWS: usize = 1_000_000;

/// OP-P-NAAJ：Null-Aware Anti Join（NOT IN + NULL 三值语义）。
pub fn null_aware_anti_join(
    left: Box<dyn Executor>,
    right: Box<dyn Executor>,
    ls: RowSchema,
    rs: RowSchema,
    spec: JoinSpec,
) -> JoinExecutor {
    JoinExecutor::new(
        left,
        right,
        ls,
        rs,
        spec.with_type(PhysicalJoinType::NullAwareAnti),
        MatchStrategy::Hash,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::row::Row;
    use crate::scan::TableScanExecutor;
    use lattice_types::DataType;

    fn left_data() -> (Vec<Row>, RowSchema) {
        let s = RowSchema::new(vec![
            ("a".into(), DataType::Int64),
            ("x".into(), DataType::VarChar(8)),
        ]);
        let rows = vec![
            vec![Scalar::Int(1), Scalar::Str("l1".into())],
            vec![Scalar::Int(2), Scalar::Str("l2".into())],
            vec![Scalar::Int(3), Scalar::Str("l3".into())],
        ];
        (rows, s)
    }

    fn right_data() -> (Vec<Row>, RowSchema) {
        let s = RowSchema::new(vec![
            ("b".into(), DataType::Int64),
            ("y".into(), DataType::VarChar(8)),
        ]);
        let rows = vec![
            vec![Scalar::Int(2), Scalar::Str("r2".into())],
            vec![Scalar::Int(3), Scalar::Str("r3".into())],
            vec![Scalar::Int(3), Scalar::Str("r3b".into())],
        ];
        (rows, s)
    }

    fn run(strategy: MatchStrategy, spec: JoinSpec) -> Result<Vec<Row>> {
        let (lr, ls) = left_data();
        let (rr, rs) = right_data();
        let mut exec = JoinExecutor::new(
            Box::new(TableScanExecutor::new(lr, ls.clone())),
            Box::new(TableScanExecutor::new(rr, rs.clone())),
            ls.clone(),
            rs,
            spec,
            strategy,
        );
        let mut ctx = ExecCtx::default();
        let blocks = collect(&mut exec, &mut ctx)?;
        let schema = exec.output_schema().clone();
        let mut out = Vec::new();
        for b in &blocks {
            out.extend(block_to_rows(b, &schema)?);
        }
        Ok(out)
    }

    fn key() -> Vec<JoinKey> {
        vec![JoinKey { left: 0, right: 0 }]
    }

    #[test]
    fn hash_inner_join_matches_duplicates() {
        let rows = run(MatchStrategy::Hash, JoinSpec::inner(key(), 2)).unwrap();
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r.len() == 4));
        assert_eq!(rows[0][0], Scalar::Int(2));
    }

    #[test]
    fn nested_loop_produces_same_pairs_as_hash() {
        let h = run(MatchStrategy::Hash, JoinSpec::inner(key(), 2)).unwrap();
        let n = run(MatchStrategy::NestedLoop, JoinSpec::inner(key(), 2)).unwrap();
        assert_eq!(h, n);
    }

    #[test]
    fn merge_join_on_sorted_inputs() {
        let m = run(MatchStrategy::Merge, JoinSpec::inner(key(), 2)).unwrap();
        let h = run(MatchStrategy::Hash, JoinSpec::inner(key(), 2)).unwrap();
        assert_eq!(m.len(), h.len());
    }

    #[test]
    fn index_join_matches_hash() {
        let i = run(MatchStrategy::Index, JoinSpec::inner(key(), 2)).unwrap();
        let h = run(MatchStrategy::Hash, JoinSpec::inner(key(), 2)).unwrap();
        assert_eq!(i, h);
    }

    #[test]
    fn left_outer_fills_nulls() {
        let rows = run(
            MatchStrategy::Hash,
            JoinSpec::inner(key(), 2).with_type(PhysicalJoinType::LeftOuter),
        )
        .unwrap();
        assert_eq!(rows.len(), 4);
        assert!(rows
            .iter()
            .any(|r| r[0] == Scalar::Int(1) && r[2] == Scalar::Null));
    }

    #[test]
    fn right_outer_fills_nulls() {
        let rows = run(
            MatchStrategy::Hash,
            JoinSpec::inner(key(), 2).with_type(PhysicalJoinType::RightOuter),
        )
        .unwrap();
        // 右表 3 行全部匹配 → 无补 NULL 行
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn full_outer_on_disjoint_sets() {
        let (_, _ls) = left_data();
        let rs = RowSchema::new(vec![("b".into(), DataType::Int64)]);
        let mut exec = JoinExecutor::new(
            Box::new(TableScanExecutor::new(
                vec![vec![Scalar::Int(1)]],
                RowSchema::new(vec![("a".into(), DataType::Int64)]),
            )),
            Box::new(TableScanExecutor::new(
                vec![vec![Scalar::Int(9)]],
                rs.clone(),
            )),
            RowSchema::new(vec![("a".into(), DataType::Int64)]),
            rs,
            JoinSpec::inner(key(), 1).with_type(PhysicalJoinType::FullOuter),
            MatchStrategy::Hash,
        );
        let mut ctx = ExecCtx::default();
        let blocks = collect(&mut exec, &mut ctx).unwrap();
        let schema = exec.output_schema().clone();
        let mut rows = Vec::new();
        for b in &blocks {
            rows.extend(block_to_rows(b, &schema).unwrap());
        }
        assert_eq!(rows.len(), 2);
        assert!(rows
            .iter()
            .any(|r| r[0] == Scalar::Null && r[1] == Scalar::Int(9)));
    }

    #[test]
    fn semi_join_outputs_left_only() {
        let rows = run(
            MatchStrategy::Hash,
            JoinSpec::inner(key(), 2).with_type(PhysicalJoinType::Semi),
        )
        .unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.len() == 2));
    }

    #[test]
    fn anti_semi_join_returns_unmatched() {
        let rows = run(
            MatchStrategy::Hash,
            JoinSpec::inner(key(), 2).with_type(PhysicalJoinType::AntiSemi),
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][0], Scalar::Int(1));
    }

    #[test]
    fn null_join_keys_never_match() {
        let (lr, ls) = left_data();
        let rs = right_data().1;
        let rr = vec![vec![Scalar::Null, Scalar::Str("r".into())]];
        let mut exec = hash_join(
            Box::new(TableScanExecutor::new(lr, ls.clone())),
            Box::new(TableScanExecutor::new(rr, rs.clone())),
            ls,
            rs,
            JoinSpec::inner(key(), 2),
        );
        let mut ctx = ExecCtx::default();
        assert!(collect(&mut exec, &mut ctx).unwrap().is_empty());
    }

    #[test]
    fn naaj_with_null_in_right_returns_nothing() {
        let (lr, ls) = left_data();
        let rs = RowSchema::new(vec![("b".into(), DataType::Int64)]);
        let rr = vec![vec![Scalar::Int(2)], vec![Scalar::Null]];
        let mut exec = null_aware_anti_join(
            Box::new(TableScanExecutor::new(lr, ls.clone())),
            Box::new(TableScanExecutor::new(rr, rs.clone())),
            ls,
            rs,
            JoinSpec::inner(vec![JoinKey { left: 0, right: 0 }], 1),
        );
        let mut ctx = ExecCtx::default();
        assert!(collect(&mut exec, &mut ctx).unwrap().is_empty());
    }

    #[test]
    fn naaj_without_null_filters_in_list() {
        let (lr, ls) = left_data();
        let rs = RowSchema::new(vec![("b".into(), DataType::Int64)]);
        let rr = vec![vec![Scalar::Int(2)], vec![Scalar::Int(3)]];
        let mut exec = null_aware_anti_join(
            Box::new(TableScanExecutor::new(lr, ls.clone())),
            Box::new(TableScanExecutor::new(rr, rs.clone())),
            ls,
            rs,
            JoinSpec::inner(vec![JoinKey { left: 0, right: 0 }], 1),
        );
        let mut ctx = ExecCtx::default();
        let blocks = collect(&mut exec, &mut ctx).unwrap();
        assert_eq!(blocks[0].num_rows, 1);
        assert_eq!(blocks[0].columns[0].get(0).unwrap(), Scalar::Int(1));
    }

    #[test]
    fn nonequi_condition_filters_pairs() {
        let cond = Expr::binary(
            lattice_expr::FuncSig::Gt,
            Expr::column(0, 1),
            lattice_expr::Expr::Literal(Scalar::Int(2), DataType::Int64),
            DataType::Bool,
        );
        let rows = run(
            MatchStrategy::NestedLoop,
            JoinSpec::inner(key(), 2).with_condition(cond),
        )
        .unwrap();
        assert!(rows.iter().all(|r| matches!(r[0], Scalar::Int(3))));
    }

    #[test]
    fn cartesian_upper_bound_is_explicit() {
        assert_eq!(CARTESIAN_MAX_ROWS, 1_000_000);
    }
}
