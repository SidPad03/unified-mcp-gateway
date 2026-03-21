use prometheus::{
    HistogramOpts, HistogramVec,
    IntCounterVec, IntGauge, IntGaugeVec, Opts, Registry,
};

pub struct MetricsCollector {
    pub registry: Registry,
    pub tool_calls_total: IntCounterVec,
    pub tool_call_duration: HistogramVec,
    pub policy_decisions_total: IntCounterVec,
    pub active_sessions: IntGauge,
    pub backend_health: IntGaugeVec,
    pub audit_events_total: IntCounterVec,
    pub bytes_in_total: IntCounterVec,
    pub bytes_out_total: IntCounterVec,
    pub rate_limit_hits: IntCounterVec,
    pub backend_restarts: IntCounterVec,
}

impl MetricsCollector {
    pub fn new() -> Self {
        let registry = Registry::new();

        let tool_calls_total = IntCounterVec::new(
            Opts::new("mcpgw_tool_calls_total", "Total tool calls"),
            &["tool", "backend", "status", "risk_category"],
        ).unwrap();

        let tool_call_duration = HistogramVec::new(
            HistogramOpts::new("mcpgw_tool_call_duration_seconds", "Tool call duration")
                .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0]),
            &["tool", "backend"],
        ).unwrap();

        let policy_decisions_total = IntCounterVec::new(
            Opts::new("mcpgw_policy_decisions_total", "Policy evaluation outcomes"),
            &["decision", "tool"],
        ).unwrap();

        let active_sessions = IntGauge::new(
            "mcpgw_active_sessions", "Currently active MCP sessions"
        ).unwrap();

        let backend_health = IntGaugeVec::new(
            Opts::new("mcpgw_backend_health", "Backend health status"),
            &["backend"],
        ).unwrap();

        let audit_events_total = IntCounterVec::new(
            Opts::new("mcpgw_audit_events_total", "Audit events written"),
            &["capture_mode"],
        ).unwrap();

        let bytes_in_total = IntCounterVec::new(
            Opts::new("mcpgw_bytes_in_total", "Total request payload bytes"),
            &["tool", "backend"],
        ).unwrap();

        let bytes_out_total = IntCounterVec::new(
            Opts::new("mcpgw_bytes_out_total", "Total response payload bytes"),
            &["tool", "backend"],
        ).unwrap();

        let rate_limit_hits = IntCounterVec::new(
            Opts::new("mcpgw_rate_limit_hits_total", "Rate limit rejections"),
            &["user", "tool"],
        ).unwrap();

        let backend_restarts = IntCounterVec::new(
            Opts::new("mcpgw_backend_restarts_total", "Backend process restarts"),
            &["backend", "reason"],
        ).unwrap();

        registry.register(Box::new(tool_calls_total.clone())).unwrap();
        registry.register(Box::new(tool_call_duration.clone())).unwrap();
        registry.register(Box::new(policy_decisions_total.clone())).unwrap();
        registry.register(Box::new(active_sessions.clone())).unwrap();
        registry.register(Box::new(backend_health.clone())).unwrap();
        registry.register(Box::new(audit_events_total.clone())).unwrap();
        registry.register(Box::new(bytes_in_total.clone())).unwrap();
        registry.register(Box::new(bytes_out_total.clone())).unwrap();
        registry.register(Box::new(rate_limit_hits.clone())).unwrap();
        registry.register(Box::new(backend_restarts.clone())).unwrap();

        Self {
            registry,
            tool_calls_total,
            tool_call_duration,
            policy_decisions_total,
            active_sessions,
            backend_health,
            audit_events_total,
            bytes_in_total,
            bytes_out_total,
            rate_limit_hits,
            backend_restarts,
        }
    }

    pub fn record_tool_call(&self, tool: &str, backend: &str, status: &str, risk: &str, duration_secs: f64) {
        self.tool_calls_total
            .with_label_values(&[tool, backend, status, risk])
            .inc();
        self.tool_call_duration
            .with_label_values(&[tool, backend])
            .observe(duration_secs);
    }

    pub fn record_policy_decision(&self, decision: &str, tool: &str) {
        self.policy_decisions_total
            .with_label_values(&[decision, tool])
            .inc();
    }
}
