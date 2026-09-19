//! Executable Route predicates (FR-12.3, FR-12.4).
//!
//! A predicate is a *deterministic, side-effect-free* expression tree over
//! Kinetix-known request/configuration facts. There is no eval, no code
//! execution, no I/O: only boolean composition and comparisons over facts.
//!
//! Evaluation is **three-valued** (`true` / `false` / `unknown`). A fact
//! Kinetix cannot establish is `unknown`, never guessed. Each evaluation also
//! produces a human-readable explanation for the Route Trace and Dry Run.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::Capabilities;

// ---------------------------------------------------------------------------
// Three-valued logic
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tri {
    True,
    False,
    Unknown,
}

impl Tri {
    pub fn from_bool(b: bool) -> Self {
        if b {
            Tri::True
        } else {
            Tri::False
        }
    }
    pub fn is_true(self) -> bool {
        matches!(self, Tri::True)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Tri::True => "true",
            Tri::False => "false",
            Tri::Unknown => "unknown",
        }
    }
}

/// How an `unknown` predicate result affects target eligibility. The admin
/// must choose; Kinetix never guesses (FR-12.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WhenUnknown {
    /// Unknown => target is not eligible (conservative, the default).
    Skip,
    /// Unknown => target remains eligible.
    Allow,
}

impl Default for WhenUnknown {
    fn default() -> Self {
        WhenUnknown::Skip
    }
}

// ---------------------------------------------------------------------------
// Facts
// ---------------------------------------------------------------------------

/// A reference to one fact Kinetix can evaluate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FactRef {
    /// `"has_tools"` — a bare fact name with no argument.
    Bare(String),
    /// `{"name": "target_capability", "arg": "vision"}` — a parameterized fact.
    Detailed {
        name: String,
        #[serde(default)]
        arg: Option<String>,
    },
}

impl FactRef {
    pub fn name(&self) -> &str {
        match self {
            FactRef::Bare(n) => n,
            FactRef::Detailed { name, .. } => name,
        }
    }
    pub fn arg(&self) -> Option<&str> {
        match self {
            FactRef::Bare(_) => None,
            FactRef::Detailed { arg, .. } => arg.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
    In,
    NotIn,
    Contains,
}

/// A predicate expression tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Predicate {
    /// `true` / `false` literal.
    Const(bool),
    /// `{"and": [..]}`
    And { and: Vec<Predicate> },
    /// `{"or": [..]}`
    Or { or: Vec<Predicate> },
    /// `{"not": {..}}`
    Not { not: Box<Predicate> },
    /// `{"fact": "has_tools", "op": "eq", "value": true}`
    Cmp {
        fact: FactRef,
        #[serde(default = "default_op")]
        op: CmpOp,
        value: Value,
    },
}

fn default_op() -> CmpOp {
    CmpOp::Eq
}

/// The stored shape of a target predicate: the expression plus the policy for
/// `unknown` results.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TargetPredicate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expr: Option<Predicate>,
    #[serde(default)]
    pub when_unknown: WhenUnknown,
}

impl TargetPredicate {
    pub fn is_empty(&self) -> bool {
        self.expr.is_none()
    }

    /// Parse from the stored JSON. An empty object (`{}`) or `null` means
    /// "always eligible".
    pub fn parse(raw: &str) -> Self {
        if raw.trim().is_empty() || raw.trim() == "{}" || raw.trim() == "null" {
            return TargetPredicate::default();
        }
        // Accept either the envelope `{expr, when_unknown}` or a bare
        // expression tree (treated as `when_unknown: skip`).
        if let Ok(tp) = serde_json::from_str::<TargetPredicate>(raw) {
            if tp.expr.is_some() {
                return tp;
            }
        }
        match serde_json::from_str::<Predicate>(raw) {
            Ok(expr) => TargetPredicate {
                expr: Some(expr),
                when_unknown: WhenUnknown::Skip,
            },
            Err(_) => TargetPredicate::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Evaluation context
// ---------------------------------------------------------------------------

/// Facts about the request itself (not target-specific).
#[derive(Debug, Clone)]
pub struct RequestFacts<'a> {
    pub frontend: &'a str,
    /// The model name the client asked for (alias, Route, or model id).
    pub requested_model: &'a str,
    /// The Route name, when the request resolved to a Route.
    pub requested_route: Option<&'a str>,
    /// The virtual key's free-form tag, when present.
    pub key_tag: Option<&'a str>,
    pub has_tools: bool,
    pub has_images: bool,
    pub has_reasoning: bool,
    pub input_tokens: u64,
}

/// Facts about one candidate target.
#[derive(Debug, Clone)]
pub struct TargetFacts<'a> {
    pub model_id: &'a str,
    pub model_display: &'a str,
    pub provider_id: &'a str,
    pub provider_name: &'a str,
    pub capabilities: &'a Capabilities,
    /// The raw capabilities JSON, used to distinguish "declared false" from
    /// "not configured" (which is `unknown`).
    pub capabilities_raw: &'a Value,
    pub context_window: Option<i64>,
    pub max_output_tokens: Option<i64>,
}

/// The outcome of evaluating a predicate: a three-valued result and the
/// explanation the Route Trace/Dry Run will show.
#[derive(Debug, Clone, Serialize)]
pub struct Eval {
    pub result: Tri,
    pub explanation: String,
}

impl Eval {
    fn known(result: bool, explanation: impl Into<String>) -> Self {
        Eval {
            result: Tri::from_bool(result),
            explanation: explanation.into(),
        }
    }
    fn unknown(explanation: impl Into<String>) -> Self {
        Eval {
            result: Tri::Unknown,
            explanation: explanation.into(),
        }
    }
}

/// Evaluate a fact to a JSON value, or `None` when the fact is unknown.
fn fact_value(fact: &FactRef, req: &RequestFacts<'_>, t: &TargetFacts<'_>) -> Option<Value> {
    match fact.name() {
        "has_tools" => Some(Value::Bool(req.has_tools)),
        "has_images" => Some(Value::Bool(req.has_images)),
        "has_reasoning" => Some(Value::Bool(req.has_reasoning)),
        "frontend" => Some(Value::String(req.frontend.to_string())),
        "requested_alias" | "requested_model" => {
            Some(Value::String(req.requested_model.to_string()))
        }
        "requested_route" => req.requested_route.map(|r| Value::String(r.to_string())),
        "key_tag" => req.key_tag.map(|t| Value::String(t.to_string())),
        "input_tokens" => Some(Value::Number(req.input_tokens.into())),
        "target_model" | "target_model_id" => Some(Value::String(t.model_id.to_string())),
        "target_provider" | "target_provider_id" => {
            Some(Value::String(t.provider_id.to_string()))
        }
        "target_capability" => {
            let cap = fact.arg()?;
            let declared = capabilities_declared(t.capabilities_raw, cap);
            if declared {
                Some(Value::Bool(capability_flag(t.capabilities, cap)))
            } else {
                None // not configured => unknown
            }
        }
        "target_context_window" => t.context_window.map(|v| Value::Number(v.into())),
        "target_max_output_tokens" => t.max_output_tokens.map(|v| Value::Number(v.into())),
        _ => None,
    }
}

fn capability_flag(caps: &Capabilities, name: &str) -> bool {
    match name {
        "text" => caps.text,
        "vision" => caps.vision,
        "reasoning" => caps.reasoning,
        "tool_calling" | "tools" => caps.tool_calling,
        "audio" => caps.audio,
        _ => false,
    }
}

/// Whether the capabilities JSON explicitly declares this capability.
fn capabilities_declared(raw: &Value, name: &str) -> bool {
    let keys: &[&str] = match name {
        "tool_calling" => &["tool_calling", "tools", "tool_calls"],
        other => &[other],
    };
    keys.iter().any(|k| raw.get(k).is_some())
}

/// Evaluate a predicate tree.
pub fn eval(pred: &Predicate, req: &RequestFacts<'_>, t: &TargetFacts<'_>) -> Eval {
    match pred {
        Predicate::Const(b) => Eval::known(*b, format!("constant {b}")),
        Predicate::And { and } => {
            let mut parts = Vec::new();
            let mut saw_unknown = false;
            for p in and {
                let e = eval(p, req, t);
                match e.result {
                    Tri::False => {
                        parts.push(format!("[{}]", e.explanation));
                        return Eval::known(false, format!("and: false because {}", parts.join(" ")));
                    }
                    Tri::Unknown => saw_unknown = true,
                    Tri::True => {}
                }
                parts.push(format!("[{}]", e.explanation));
            }
            if saw_unknown {
                Eval::unknown(format!("and: unknown because {}", parts.join(" ")))
            } else {
                Eval::known(true, format!("and: all true {}", parts.join(" ")))
            }
        }
        Predicate::Or { or } => {
            let mut parts = Vec::new();
            let mut saw_unknown = false;
            for p in or {
                let e = eval(p, req, t);
                match e.result {
                    Tri::True => {
                        parts.push(format!("[{}]", e.explanation));
                        return Eval::known(true, format!("or: true because {}", parts.join(" ")));
                    }
                    Tri::Unknown => saw_unknown = true,
                    Tri::False => {}
                }
                parts.push(format!("[{}]", e.explanation));
            }
            if saw_unknown {
                Eval::unknown(format!("or: unknown because {}", parts.join(" ")))
            } else {
                Eval::known(false, format!("or: all false {}", parts.join(" ")))
            }
        }
        Predicate::Not { not } => {
            let e = eval(not, req, t);
            let result = match e.result {
                Tri::True => Tri::False,
                Tri::False => Tri::True,
                Tri::Unknown => Tri::Unknown,
            };
            Eval {
                result,
                explanation: format!("not [{}]", e.explanation),
            }
        }
        Predicate::Cmp { fact, op, value } => eval_cmp(fact, *op, value, req, t),
    }
}

fn eval_cmp(
    fact: &FactRef,
    op: CmpOp,
    expected: &Value,
    req: &RequestFacts<'_>,
    t: &TargetFacts<'_>,
) -> Eval {
    let Some(actual) = fact_value(fact, req, t) else {
        return Eval::unknown(format!(
            "fact '{}'{} is unknown",
            fact.name(),
            fact.arg().map(|a| format!("({a})")).unwrap_or_default()
        ));
    };
    let label = format!(
        "{}{} {} {}",
        fact.name(),
        fact.arg().map(|a| format!("({a})")).unwrap_or_default(),
        op_str(op),
        compact(expected)
    );
    let result = match op {
        CmpOp::Eq => json_eq(&actual, expected),
        CmpOp::Ne => !json_eq(&actual, expected),
        CmpOp::Lt | CmpOp::Lte | CmpOp::Gt | CmpOp::Gte => {
            let (Some(a), Some(b)) = (as_f64(&actual), as_f64(expected)) else {
                return Eval::unknown(format!("{label}: not comparable (non-numeric)"));
            };
            match op {
                CmpOp::Lt => a < b,
                CmpOp::Lte => a <= b,
                CmpOp::Gt => a > b,
                CmpOp::Gte => a >= b,
                _ => unreachable!(),
            }
        }
        CmpOp::In | CmpOp::NotIn => {
            let Some(arr) = expected.as_array() else {
                return Eval::unknown(format!("{label}: 'in' value must be an array"));
            };
            let contains = arr.iter().any(|v| json_eq(&actual, v));
            if op == CmpOp::In {
                contains
            } else {
                !contains
            }
        }
        CmpOp::Contains => match (actual, expected) {
            (Value::String(s), Value::String(sub)) => s.contains(sub.as_str()),
            (Value::Array(a), v) => a.iter().any(|x| json_eq(x, v)),
            _ => return Eval::unknown(format!("{label}: 'contains' needs a string or array")),
        },
    };
    Eval::known(result, format!("{label} => {result}"))
}

fn op_str(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Eq => "==",
        CmpOp::Ne => "!=",
        CmpOp::Lt => "<",
        CmpOp::Lte => "<=",
        CmpOp::Gt => ">",
        CmpOp::Gte => ">=",
        CmpOp::In => "in",
        CmpOp::NotIn => "not in",
        CmpOp::Contains => "contains",
    }
}

fn json_eq(a: &Value, b: &Value) -> bool {
    if a == b {
        return true;
    }
    // Numeric cross-type equality (1 == 1.0).
    if let (Some(x), Some(y)) = (as_f64(a), as_f64(b)) {
        return (x - y).abs() < f64::EPSILON;
    }
    false
}

fn as_f64(v: &Value) -> Option<f64> {
    v.as_f64()
}

fn compact(v: &Value) -> String {
    let s = v.to_string();
    if s.len() > 60 {
        format!("{}…", &s[..60])
    } else {
        s
    }
}

/// The full eligibility decision for one target, ready for the Route Trace.
#[derive(Debug, Clone, Serialize)]
pub struct Eligibility {
    pub eligible: bool,
    pub result: Tri,
    pub explanation: String,
}

/// Decide eligibility from a target predicate.
pub fn eligibility(
    pred: &TargetPredicate,
    req: &RequestFacts<'_>,
    t: &TargetFacts<'_>,
) -> Eligibility {
    let Some(expr) = &pred.expr else {
        return Eligibility {
            eligible: true,
            result: Tri::True,
            explanation: "no predicate configured".to_string(),
        };
    };
    let e = eval(expr, req, t);
    let eligible = match e.result {
        Tri::True => true,
        Tri::False => false,
        Tri::Unknown => pred.when_unknown == WhenUnknown::Allow,
    };
    Eligibility {
        eligible,
        result: e.result,
        explanation: format!(
            "{} (when_unknown={})",
            e.explanation,
            match pred.when_unknown {
                WhenUnknown::Skip => "skip",
                WhenUnknown::Allow => "allow",
            }
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn req<'a>(tools: bool) -> RequestFacts<'a> {
        RequestFacts {
            frontend: "openai",
            requested_model: "coder",
            requested_route: None,
            key_tag: Some("dev"),
            has_tools: tools,
            has_images: false,
            has_reasoning: false,
            input_tokens: 100,
        }
    }

    fn target<'a>(caps: &'a Capabilities, raw: &'a Value) -> TargetFacts<'a> {
        TargetFacts {
            model_id: "m1",
            model_display: "Model One",
            provider_id: "p1",
            provider_name: "Provider One",
            capabilities: caps,
            capabilities_raw: raw,
            context_window: Some(128_000),
            max_output_tokens: None,
        }
    }

    #[test]
    fn three_valued_and() {
        let p: Predicate = serde_json::from_value(json!({
            "and": [
                {"fact": "has_tools", "op": "eq", "value": true},
                {"fact": "has_images", "op": "eq", "value": false}
            ]
        }))
        .unwrap();
        let caps = Capabilities::default();
        let raw = json!({});
        assert!(eval(&p, &req(true), &target(&caps, &raw)).result.is_true());
        assert_eq!(eval(&p, &req(false), &target(&caps, &raw)).result, Tri::False);
    }

    #[test]
    fn unknown_capability_is_unknown_then_skipped() {
        let pred = TargetPredicate {
            expr: Some(
                serde_json::from_value(json!({
                    "fact": {"name": "target_capability", "arg": "vision"},
                    "op": "eq",
                    "value": true
                }))
                .unwrap(),
            ),
            when_unknown: WhenUnknown::Skip,
        };
        let caps = Capabilities::default();
        let raw = json!({}); // vision not declared
        let e = eligibility(&pred, &req(true), &target(&caps, &raw));
        assert_eq!(e.result, Tri::Unknown);
        assert!(!e.eligible);
    }

    #[test]
    fn declared_capability_is_known() {
        let pred = TargetPredicate {
            expr: Some(
                serde_json::from_value(json!({
                    "fact": {"name": "target_capability", "arg": "vision"},
                    "op": "eq",
                    "value": true
                }))
                .unwrap(),
            ),
            when_unknown: WhenUnknown::Skip,
        };
        let caps = Capabilities { vision: true, ..Default::default() };
        let raw = json!({"vision": true});
        let e = eligibility(&pred, &req(true), &target(&caps, &raw));
        assert!(e.eligible);
    }

    #[test]
    fn numeric_comparison() {
        let p: Predicate = serde_json::from_value(json!({
            "fact": "input_tokens", "op": "lt", "value": 200
        }))
        .unwrap();
        let caps = Capabilities::default();
        let raw = json!({});
        assert!(eval(&p, &req(true), &target(&caps, &raw)).result.is_true());
    }
}
