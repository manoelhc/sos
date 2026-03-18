#[cfg(feature = "std")]
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PfError {
    Io(String),
    Yaml(String),
    Json(String),
    Schema(String),
    Command(String),
}

#[cfg(feature = "std")]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PfConfig {
    #[serde(rename = "sos-pf")]
    pub sos_pf: PfRoot,
}

#[cfg(feature = "std")]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PfRoot {
    pub tables: Vec<PfTable>,
}

#[cfg(feature = "std")]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PfTable {
    pub name: String,
    pub family: String,
    #[serde(default)]
    pub sets: Vec<PfSet>,
    #[serde(default)]
    pub maps: Vec<PfMap>,
    pub chains: Vec<PfChain>,
}

#[cfg(feature = "std")]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PfSet {
    pub name: String,
    #[serde(rename = "type")]
    pub elem_type: String,
    #[serde(default)]
    pub elements: Vec<String>,
}

#[cfg(feature = "std")]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PfMap {
    pub name: String,
    #[serde(rename = "type")]
    pub key_type: String,
    #[serde(rename = "value_type")]
    pub value_type: String,
    #[serde(default)]
    pub elements: Vec<PfMapElement>,
}

#[cfg(feature = "std")]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PfMapElement {
    pub key: String,
    pub value: String,
}

#[cfg(feature = "std")]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PfChain {
    pub name: String,
    #[serde(rename = "type")]
    pub chain_type: String,
    pub hook: String,
    pub priority: i32,
    pub policy: String,
    pub rules: Vec<PfRule>,
}

#[cfg(feature = "std")]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PfRule {
    #[serde(default)]
    pub match_expr: Option<PfMatch>,
    pub action: String,
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default)]
    pub rate: Option<String>,
    #[serde(default)]
    pub burst: Option<u32>,
    #[serde(default)]
    pub comment: Option<String>,
}

#[cfg(feature = "std")]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PfMatch {
    #[serde(default)]
    pub ct: Option<PfConntrackMatch>,
    #[serde(default)]
    pub ip: Option<PfIpMatch>,
    #[serde(default)]
    pub ip6: Option<PfIpMatch>,
    #[serde(default)]
    pub tcp: Option<PfTcpUdpMatch>,
    #[serde(default)]
    pub udp: Option<PfTcpUdpMatch>,
    #[serde(default)]
    pub icmp: Option<PfIcmpMatch>,
    #[serde(default)]
    pub sctp: Option<PfTcpUdpMatch>,
    #[serde(default)]
    pub set: Option<PfSetRefMatch>,
}

#[cfg(feature = "std")]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PfConntrackMatch {
    pub state: Vec<String>,
}

#[cfg(feature = "std")]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PfIpMatch {
    #[serde(default)]
    pub saddr: Option<String>,
    #[serde(default)]
    pub daddr: Option<String>,
    #[serde(default)]
    pub protocol: Option<String>,
}

#[cfg(feature = "std")]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PfTcpUdpMatch {
    #[serde(default)]
    pub sport: Option<u16>,
    #[serde(default)]
    pub dport: Option<u16>,
}

#[cfg(feature = "std")]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PfIcmpMatch {
    #[serde(default)]
    pub icmp_type: Option<String>,
}

#[cfg(feature = "std")]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PfSetRefMatch {
    pub name: String,
    pub field: String,
}

#[cfg(feature = "std")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplyPlan {
    pub script: String,
}

#[cfg(feature = "std")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PfCheckReport {
    pub kernel_support_ok: bool,
    pub nft_parse_ok: bool,
}

#[cfg(feature = "std")]
pub trait NftRunner {
    fn run_nft(&self, args: &[&str], stdin_script: Option<&str>) -> Result<String, PfError>;
}

#[cfg(feature = "std")]
pub struct SystemNftRunner;

#[cfg(feature = "std")]
impl NftRunner for SystemNftRunner {
    fn run_nft(&self, args: &[&str], stdin_script: Option<&str>) -> Result<String, PfError> {
        use std::io::Write;
        use std::process::{Command, Stdio};

        let mut cmd = Command::new("nft");
        cmd.args(args);
        if stdin_script.is_some() {
            cmd.stdin(Stdio::piped());
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| PfError::Command(format!("failed to spawn nft: {e}")))?;

        if let Some(script) = stdin_script {
            let stdin = child
                .stdin
                .as_mut()
                .ok_or_else(|| PfError::Command("failed to open nft stdin".to_string()))?;
            stdin
                .write_all(script.as_bytes())
                .map_err(|e| PfError::Command(format!("failed writing nft stdin: {e}")))?;
        }

        let out = child
            .wait_with_output()
            .map_err(|e| PfError::Command(format!("failed waiting nft: {e}")))?;

        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).to_string())
        } else {
            Err(PfError::Command(
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ))
        }
    }
}

#[cfg(feature = "std")]
pub fn parse_config(yaml: &str) -> Result<PfConfig, PfError> {
    let cfg: PfConfig = serde_yaml::from_str(yaml).map_err(|e| PfError::Yaml(e.to_string()))?;
    validate_config(&cfg)?;
    Ok(cfg)
}

#[cfg(feature = "std")]
pub fn check_config(yaml: &str) -> Result<(), PfError> {
    let _ = parse_config(yaml)?;
    Ok(())
}

#[cfg(feature = "std")]
pub fn export_config_yaml(config: &PfConfig) -> Result<String, PfError> {
    serde_yaml::to_string(config).map_err(|e| PfError::Yaml(e.to_string()))
}

#[cfg(feature = "std")]
pub fn build_apply_plan(config: &PfConfig) -> Result<ApplyPlan, PfError> {
    validate_config(config)?;
    let mut script = String::from("flush ruleset\n");

    for table in &config.sos_pf.tables {
        script.push_str(&format!("add table {} {}\n", table.family, table.name));

        for set in &table.sets {
            script.push_str(&format!(
                "add set {} {} {} {{ type {}; }}\n",
                table.family, table.name, set.name, set.elem_type
            ));
            for elem in &set.elements {
                script.push_str(&format!(
                    "add element {} {} {} {{ {} }}\n",
                    table.family, table.name, set.name, elem
                ));
            }
        }

        for map in &table.maps {
            script.push_str(&format!(
                "add map {} {} {} {{ type {} : {}; }}\n",
                table.family, table.name, map.name, map.key_type, map.value_type
            ));
            for elem in &map.elements {
                script.push_str(&format!(
                    "add element {} {} {} {{ {} : {} }}\n",
                    table.family, table.name, map.name, elem.key, elem.value
                ));
            }
        }

        for chain in &table.chains {
            script.push_str(&format!(
                "add chain {} {} {} {{ type {} hook {} priority {}; policy {}; }}\n",
                table.family,
                table.name,
                chain.name,
                chain.chain_type,
                chain.hook,
                chain.priority,
                chain.policy
            ));

            for rule in &chain.rules {
                let expr = render_rule_expr(table, rule)?;
                script.push_str(&format!(
                    "add rule {} {} {} {}\n",
                    table.family, table.name, chain.name, expr
                ));
            }
        }
    }

    Ok(ApplyPlan { script })
}

#[cfg(feature = "std")]
pub fn dry_run_check_with_runner(
    yaml: &str,
    runner: &dyn NftRunner,
) -> Result<PfCheckReport, PfError> {
    let cfg = parse_config(yaml)?;
    let plan = build_apply_plan(&cfg)?;

    let _ = runner.run_nft(&["--version"], None)?;
    let _ = runner.run_nft(&["-c", "-f", "-"], Some(&plan.script))?;

    Ok(PfCheckReport {
        kernel_support_ok: true,
        nft_parse_ok: true,
    })
}

#[cfg(feature = "std")]
pub fn apply_with_runner(yaml: &str, runner: &dyn NftRunner) -> Result<(), PfError> {
    let cfg = parse_config(yaml)?;
    let plan = build_apply_plan(&cfg)?;
    let _ = runner.run_nft(&["-f", "-"], Some(&plan.script))?;
    Ok(())
}

#[cfg(feature = "std")]
pub fn export_running_ruleset_yaml_with_runner(runner: &dyn NftRunner) -> Result<String, PfError> {
    let json = runner.run_nft(&["-j", "list", "ruleset"], None)?;
    ruleset_json_to_yaml(&json)
}

#[cfg(feature = "std")]
pub fn ruleset_json_to_yaml(json: &str) -> Result<String, PfError> {
    #[derive(Deserialize)]
    struct NftWrapper {
        nftables: Vec<serde_json::Value>,
    }

    let parsed: NftWrapper =
        serde_json::from_str(json).map_err(|e| PfError::Json(e.to_string()))?;

    let mut cfg = PfConfig {
        sos_pf: PfRoot { tables: Vec::new() },
    };

    for item in &parsed.nftables {
        if let Some(tbl) = item.get("table") {
            let family = str_field(tbl, "family")?;
            let name = str_field(tbl, "name")?;
            if cfg
                .sos_pf
                .tables
                .iter()
                .all(|t| !(t.family == family && t.name == name))
            {
                cfg.sos_pf.tables.push(PfTable {
                    name,
                    family,
                    sets: Vec::new(),
                    maps: Vec::new(),
                    chains: Vec::new(),
                });
            }
        }
    }

    for item in &parsed.nftables {
        if let Some(chain) = item.get("chain") {
            let family = str_field(chain, "family")?;
            let table = str_field(chain, "table")?;
            let name = str_field(chain, "name")?;

            let chain_type = chain
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("filter")
                .to_string();
            let hook = chain
                .get("hook")
                .and_then(|v| v.as_str())
                .unwrap_or("input")
                .to_string();
            let priority = chain.get("prio").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let policy = chain
                .get("policy")
                .and_then(|v| v.as_str())
                .unwrap_or("accept")
                .to_string();

            if let Some(tbl) = cfg
                .sos_pf
                .tables
                .iter_mut()
                .find(|t| t.family == family && t.name == table)
            {
                tbl.chains.push(PfChain {
                    name,
                    chain_type,
                    hook,
                    priority,
                    policy,
                    rules: Vec::new(),
                });
            }
        }
    }

    export_config_yaml(&cfg)
}

#[cfg(feature = "std")]
fn str_field(value: &serde_json::Value, key: &str) -> Result<String, PfError> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
        .ok_or_else(|| PfError::Json(format!("missing or invalid field '{key}'")))
}

#[cfg(feature = "std")]
fn validate_config(cfg: &PfConfig) -> Result<(), PfError> {
    if cfg.sos_pf.tables.is_empty() {
        return Err(PfError::Schema(
            "at least one table is required".to_string(),
        ));
    }

    for table in &cfg.sos_pf.tables {
        if table.name.trim().is_empty() {
            return Err(PfError::Schema("table name cannot be empty".to_string()));
        }
        validate_family(&table.family)?;

        for set in &table.sets {
            validate_set_type(&set.elem_type)?;
        }

        for map in &table.maps {
            validate_set_type(&map.key_type)?;
            validate_map_value_type(&map.value_type)?;
        }

        if table.chains.is_empty() {
            return Err(PfError::Schema(format!(
                "table '{}' must include at least one chain",
                table.name
            )));
        }

        for chain in &table.chains {
            if chain.name.trim().is_empty() {
                return Err(PfError::Schema("chain name cannot be empty".to_string()));
            }
            validate_chain_type(&chain.chain_type)?;
            validate_hook(&chain.hook)?;
            validate_policy(&chain.policy)?;

            for rule in &chain.rules {
                validate_action(&rule.action)?;
                validate_rule(rule, table)?;
            }
        }
    }
    Ok(())
}

#[cfg(feature = "std")]
fn validate_set_type(elem_type: &str) -> Result<(), PfError> {
    match elem_type {
        "ipv4_addr" | "ipv6_addr" | "inet_service" | "ifname" | "mark" => Ok(()),
        other => Err(PfError::Schema(format!("unsupported set type '{other}'"))),
    }
}

#[cfg(feature = "std")]
fn validate_map_value_type(value_type: &str) -> Result<(), PfError> {
    match value_type {
        "verdict" | "ipv4_addr" | "ipv6_addr" | "inet_service" => Ok(()),
        other => Err(PfError::Schema(format!(
            "unsupported map value type '{other}'"
        ))),
    }
}

#[cfg(feature = "std")]
fn validate_family(family: &str) -> Result<(), PfError> {
    match family {
        "ip" | "ip6" | "inet" | "arp" | "bridge" | "netdev" => Ok(()),
        other => Err(PfError::Schema(format!("unsupported family '{other}'"))),
    }
}

#[cfg(feature = "std")]
fn validate_chain_type(chain_type: &str) -> Result<(), PfError> {
    match chain_type {
        "filter" | "nat" | "route" => Ok(()),
        other => Err(PfError::Schema(format!("unsupported chain type '{other}'"))),
    }
}

#[cfg(feature = "std")]
fn validate_hook(hook: &str) -> Result<(), PfError> {
    match hook {
        "prerouting" | "input" | "forward" | "output" | "postrouting" => Ok(()),
        other => Err(PfError::Schema(format!("unsupported hook '{other}'"))),
    }
}

#[cfg(feature = "std")]
fn validate_policy(policy: &str) -> Result<(), PfError> {
    match policy {
        "accept" | "drop" => Ok(()),
        other => Err(PfError::Schema(format!("unsupported policy '{other}'"))),
    }
}

#[cfg(feature = "std")]
fn validate_action(action: &str) -> Result<(), PfError> {
    match action {
        "accept" | "drop" | "reject" | "log" | "snat" | "dnat" | "masquerade" | "redirect"
        | "limit" => Ok(()),
        other => Err(PfError::Schema(format!("unsupported action '{other}'"))),
    }
}

#[cfg(feature = "std")]
fn validate_rule(rule: &PfRule, table: &PfTable) -> Result<(), PfError> {
    if matches!(rule.action.as_str(), "snat" | "dnat" | "redirect") && rule.to.is_none() {
        return Err(PfError::Schema(format!(
            "action '{}' requires 'to' field",
            rule.action
        )));
    }

    if rule.action == "limit" && rule.rate.is_none() {
        return Err(PfError::Schema("limit action requires rate".to_string()));
    }

    if let Some(m) = &rule.match_expr {
        if let Some(ct) = &m.ct {
            for state in &ct.state {
                if !matches!(
                    state.as_str(),
                    "new" | "established" | "related" | "invalid"
                ) {
                    return Err(PfError::Schema(format!(
                        "unsupported conntrack state '{}'",
                        state
                    )));
                }
            }
        }

        if let Some(set_ref) = &m.set {
            if table.sets.iter().all(|s| s.name != set_ref.name) {
                return Err(PfError::Schema(format!(
                    "rule references unknown set '{}'",
                    set_ref.name
                )));
            }
        }
    }

    Ok(())
}

#[cfg(feature = "std")]
fn render_rule_expr(table: &PfTable, rule: &PfRule) -> Result<String, PfError> {
    let mut parts: Vec<String> = Vec::new();

    if let Some(m) = &rule.match_expr {
        if let Some(ct) = &m.ct {
            parts.push(format!("ct state {{ {} }}", ct.state.join(", ")));
        }

        if let Some(ip) = &m.ip {
            if let Some(saddr) = &ip.saddr {
                parts.push(format!("ip saddr {}", saddr));
            }
            if let Some(daddr) = &ip.daddr {
                parts.push(format!("ip daddr {}", daddr));
            }
            if let Some(proto) = &ip.protocol {
                parts.push(format!("ip protocol {}", proto));
            }
        }

        if let Some(ip6) = &m.ip6 {
            if let Some(saddr) = &ip6.saddr {
                parts.push(format!("ip6 saddr {}", saddr));
            }
            if let Some(daddr) = &ip6.daddr {
                parts.push(format!("ip6 daddr {}", daddr));
            }
            if let Some(proto) = &ip6.protocol {
                parts.push(format!("ip6 nexthdr {}", proto));
            }
        }

        if let Some(tcp) = &m.tcp {
            if let Some(sport) = tcp.sport {
                parts.push(format!("tcp sport {}", sport));
            }
            if let Some(dport) = tcp.dport {
                parts.push(format!("tcp dport {}", dport));
            }
        }

        if let Some(udp) = &m.udp {
            if let Some(sport) = udp.sport {
                parts.push(format!("udp sport {}", sport));
            }
            if let Some(dport) = udp.dport {
                parts.push(format!("udp dport {}", dport));
            }
        }

        if let Some(sctp) = &m.sctp {
            if let Some(sport) = sctp.sport {
                parts.push(format!("sctp sport {}", sport));
            }
            if let Some(dport) = sctp.dport {
                parts.push(format!("sctp dport {}", dport));
            }
        }

        if let Some(icmp) = &m.icmp {
            if let Some(icmp_type) = &icmp.icmp_type {
                parts.push(format!("icmp type {}", icmp_type));
            }
        }

        if let Some(set_ref) = &m.set {
            let field = match set_ref.field.as_str() {
                "ip.saddr" => "ip saddr",
                "ip.daddr" => "ip daddr",
                "ip6.saddr" => "ip6 saddr",
                "ip6.daddr" => "ip6 daddr",
                "tcp.dport" => "tcp dport",
                "udp.dport" => "udp dport",
                other => {
                    return Err(PfError::Schema(format!(
                        "unsupported set match field '{}'",
                        other
                    )));
                }
            };

            if table.sets.iter().all(|s| s.name != set_ref.name) {
                return Err(PfError::Schema(format!(
                    "rule references unknown set '{}'",
                    set_ref.name
                )));
            }
            parts.push(format!("{} @{}", field, set_ref.name));
        }
    }

    let action_expr = match rule.action.as_str() {
        "accept" | "drop" | "reject" | "log" | "masquerade" => rule.action.clone(),
        "snat" | "dnat" | "redirect" => format!(
            "{} to {}",
            rule.action,
            rule.to
                .clone()
                .ok_or_else(|| PfError::Schema("missing to".to_string()))?
        ),
        "limit" => {
            let rate = rule
                .rate
                .clone()
                .ok_or_else(|| PfError::Schema("missing rate".to_string()))?;
            if let Some(burst) = rule.burst {
                format!("limit rate {} burst {} packets accept", rate, burst)
            } else {
                format!("limit rate {} accept", rate)
            }
        }
        other => return Err(PfError::Schema(format!("unsupported action '{other}'"))),
    };

    parts.push(action_expr);

    if let Some(comment) = &rule.comment {
        parts.push(format!("comment \"{}\"", escape_comment(comment)));
    }

    Ok(parts.join(" "))
}

#[cfg(feature = "std")]
fn escape_comment(comment: &str) -> String {
    comment.replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_YAML: &str = r#"
sos-pf:
  tables:
    - name: filter_table
      family: inet
      sets:
        - name: blacklist
          type: ipv4_addr
          elements: ["10.0.0.10", "10.0.0.11"]
      maps:
        - name: svc_map
          type: inet_service
          value_type: verdict
          elements:
            - key: "22"
              value: accept
      chains:
        - name: input_filter
          type: filter
          hook: input
          priority: 0
          policy: drop
          rules:
            - match_expr:
                ct:
                  state: [established, related]
              action: accept
            - match_expr:
                tcp:
                  dport: 22
              action: accept
              comment: Allow SSH
            - match_expr:
                set:
                  name: blacklist
                  field: ip.saddr
              action: drop
            - action: limit
              rate: "25/second"
              burst: 100
"#;

    const NAT_YAML: &str = r#"
sos-pf:
  tables:
    - name: nat_table
      family: inet
      chains:
        - name: preroute_nat
          type: nat
          hook: prerouting
          priority: 0
          policy: accept
          rules:
            - match_expr:
                tcp:
                  dport: 443
              action: dnat
              to: 10.0.0.20:8443
"#;

    #[cfg(feature = "std")]
    type RunnerExpectation = (Vec<String>, Option<String>, Result<String, PfError>);

    #[cfg(feature = "std")]
    struct FakeRunner {
        expected: Vec<RunnerExpectation>,
        calls: std::sync::Mutex<Vec<(Vec<String>, Option<String>)>>,
    }

    #[cfg(feature = "std")]
    impl FakeRunner {
        fn new(expected: Vec<RunnerExpectation>) -> Self {
            Self {
                expected,
                calls: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    #[cfg(feature = "std")]
    impl NftRunner for FakeRunner {
        fn run_nft(&self, args: &[&str], stdin_script: Option<&str>) -> Result<String, PfError> {
            let args_vec: Vec<String> = args.iter().map(|x| (*x).to_string()).collect();
            let stdin_vec = stdin_script.map(ToOwned::to_owned);
            self.calls
                .lock()
                .expect("lock calls")
                .push((args_vec.clone(), stdin_vec.clone()));

            for (e_args, e_stdin, result) in &self.expected {
                if *e_args == args_vec && *e_stdin == stdin_vec {
                    return result.clone();
                }
            }

            Err(PfError::Command("unexpected call".to_string()))
        }
    }

    #[cfg(feature = "std")]
    #[test]
    fn parse_valid_yaml_config() {
        let cfg = parse_config(VALID_YAML).expect("valid yaml should parse");
        assert_eq!(cfg.sos_pf.tables.len(), 1);
        assert_eq!(cfg.sos_pf.tables[0].family, "inet");
        assert_eq!(cfg.sos_pf.tables[0].chains.len(), 1);
        assert_eq!(cfg.sos_pf.tables[0].chains[0].rules.len(), 4);
        assert_eq!(cfg.sos_pf.tables[0].sets.len(), 1);
        assert_eq!(cfg.sos_pf.tables[0].maps.len(), 1);
    }

    #[cfg(feature = "std")]
    #[test]
    fn reject_unknown_table_family() {
        let invalid = VALID_YAML.replace("family: inet", "family: banana");
        let err = check_config(&invalid).expect_err("unknown family must fail");
        match err {
            PfError::Schema(msg) => assert!(msg.contains("family")),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[cfg(feature = "std")]
    #[test]
    fn reject_unknown_set_reference() {
        let invalid = VALID_YAML.replace(
            "name: blacklist\n                  field: ip.saddr",
            "name: no-such-set\n                  field: ip.saddr",
        );
        let err = parse_config(&invalid).expect_err("unknown set ref must fail");
        match err {
            PfError::Schema(msg) => assert!(msg.contains("unknown set")),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[cfg(feature = "std")]
    #[test]
    fn reject_nat_without_to() {
        let invalid = NAT_YAML.replace("to: 10.0.0.20:8443\n", "");
        let err = parse_config(&invalid).expect_err("dnat without to must fail");
        match err {
            PfError::Schema(msg) => assert!(msg.contains("requires 'to'")),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[cfg(feature = "std")]
    #[test]
    fn build_apply_plan_contains_expected_nft_statements() {
        let cfg = parse_config(VALID_YAML).expect("parse");
        let plan = build_apply_plan(&cfg).expect("plan");

        assert!(plan.script.contains("flush ruleset"));
        assert!(plan.script.contains("add table inet filter_table"));
        assert!(
            plan.script
                .contains("add chain inet filter_table input_filter { type filter hook input priority 0; policy drop; }")
        );
        assert!(plan
            .script
            .contains("ct state { established, related } accept"));
        assert!(plan
            .script
            .contains("tcp dport 22 accept comment \"Allow SSH\""));
        assert!(plan.script.contains("ip saddr @blacklist drop"));
        assert!(plan
            .script
            .contains("limit rate 25/second burst 100 packets accept"));
    }

    #[cfg(feature = "std")]
    #[test]
    fn build_apply_plan_renders_nat_action() {
        let cfg = parse_config(NAT_YAML).expect("parse nat");
        let plan = build_apply_plan(&cfg).expect("plan nat");
        assert!(plan.script.contains("tcp dport 443 dnat to 10.0.0.20:8443"));
    }

    #[cfg(feature = "std")]
    #[test]
    fn dry_run_uses_version_and_check_mode() {
        let cfg = parse_config(VALID_YAML).expect("parse");
        let plan = build_apply_plan(&cfg).expect("plan");
        let expected_script = plan.script.clone();

        let runner = FakeRunner::new(vec![
            (
                vec!["--version".to_string()],
                None,
                Ok("nftables v1".to_string()),
            ),
            (
                vec!["-c".to_string(), "-f".to_string(), "-".to_string()],
                Some(expected_script),
                Ok(String::new()),
            ),
        ]);

        let report = dry_run_check_with_runner(VALID_YAML, &runner).expect("dry run ok");
        assert!(report.kernel_support_ok);
        assert!(report.nft_parse_ok);
    }

    #[cfg(feature = "std")]
    #[test]
    fn apply_uses_atomic_script() {
        let cfg = parse_config(VALID_YAML).expect("parse");
        let plan = build_apply_plan(&cfg).expect("plan");
        let expected_script = plan.script.clone();

        let runner = FakeRunner::new(vec![(
            vec!["-f".to_string(), "-".to_string()],
            Some(expected_script),
            Ok(String::new()),
        )]);

        apply_with_runner(VALID_YAML, &runner).expect("apply ok");
    }

    #[cfg(feature = "std")]
    #[test]
    fn export_ruleset_json_to_yaml_minimal() {
        let json = r#"{
  "nftables": [
    {"metainfo":{"json_schema_version":1}},
    {"table":{"family":"inet","name":"filter_table"}},
    {"chain":{"family":"inet","table":"filter_table","name":"input_filter","type":"filter","hook":"input","prio":0,"policy":"drop"}}
  ]
}"#;

        let yaml = ruleset_json_to_yaml(json).expect("json->yaml");
        assert!(yaml.contains("sos-pf:"));
        assert!(yaml.contains("filter_table"));
        assert!(yaml.contains("input_filter"));
    }

    #[cfg(feature = "std")]
    #[test]
    fn export_round_trip_contains_sos_pf_key() {
        let cfg = parse_config(VALID_YAML).expect("parse");
        let out = export_config_yaml(&cfg).expect("export");
        assert!(out.contains("sos-pf:"));
        assert!(out.contains("filter_table"));
    }
}
