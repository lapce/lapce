//! Security Scanner - Detects sensitive information and vulnerabilities.
//!
//! This module provides:
//! - Secret detection
//! - Vulnerability scanning with semantic analysis
//! - Security best practice checks
//! - Compliance reporting
//! - AST-based code analysis
//! - Data flow tracking

use std::collections::{HashMap, HashSet};
use regex::Regex;

/// A security finding.
#[derive(Debug, Clone)]
pub struct SecurityFinding {
    pub id: String,
    pub severity: SecuritySeverity,
    pub category: SecurityCategory,
    pub title: String,
    pub description: String,
    pub file: String,
    pub line: usize,
    pub code_snippet: String,
    pub recommendation: String,
    pub cwe_id: Option<String>,
}

/// Security severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SecuritySeverity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

impl SecuritySeverity {
    pub fn score(&self) -> u32 {
        match self {
            SecuritySeverity::Critical => 10,
            SecuritySeverity::High => 7,
            SecuritySeverity::Medium => 5,
            SecuritySeverity::Low => 3,
            SecuritySeverity::Info => 1,
        }
    }

    pub fn from_score(score: u32) -> Self {
        if score >= 9 { SecuritySeverity::Critical }
        else if score >= 6 { SecuritySeverity::High }
        else if score >= 4 { SecuritySeverity::Medium }
        else if score >= 2 { SecuritySeverity::Low }
        else { SecuritySeverity::Info }
    }
}

/// Security category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecurityCategory {
    Secret,
    Injection,
    Cryptography,
    Authentication,
    Authorization,
    Privacy,
    Configuration,
    Dependency,
    InsecureDeserialization,
   XXE,
    Cors,
    SSRF,
    RaceCondition,
    MemorySafety,
}

impl SecurityCategory {
    pub fn display_name(&self) -> &str {
        match self {
            SecurityCategory::Secret => "Sensitive Information Exposure",
            SecurityCategory::Injection => "Injection Vulnerability",
            SecurityCategory::Cryptography => "Cryptographic Issue",
            SecurityCategory::Authentication => "Authentication Issue",
            SecurityCategory::Authorization => "Authorization Issue",
            SecurityCategory::Privacy => "Privacy Violation",
            SecurityCategory::Configuration => "Configuration Issue",
            SecurityCategory::Dependency => "Dependency Vulnerability",
            SecurityCategory::InsecureDeserialization => "Insecure Deserialization",
            SecurityCategory::XXE => "XML External Entity (XXE)",
            SecurityCategory::Cors => "CORS Misconfiguration",
            SecurityCategory::SSRF => "Server-Side Request Forgery",
            SecurityCategory::RaceCondition => "Race Condition",
            SecurityCategory::MemorySafety => "Memory Safety Issue",
        }
    }
}

/// Semantic context for code analysis.
#[derive(Debug, Clone)]
pub struct SemanticContext {
    pub file_path: String,
    pub language: String,
    pub function_name: Option<String>,
    pub class_name: Option<String>,
    pub variables: HashMap<String, VariableInfo>,
    pub imports: Vec<String>,
    pub current_scope: Scope,
}

#[derive(Debug, Clone)]
pub struct VariableInfo {
    pub name: String,
    pub var_type: String,
    pub is_sensitive: bool,
    pub source: Option<String>,
    pub sink: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Scope {
    pub depth: usize,
    pub parent: Option<Box<Scope>>,
    pub variables: HashSet<String>,
}

impl Scope {
    pub fn new() -> Self {
        Self {
            depth: 0,
            parent: None,
            variables: HashSet::new(),
        }
    }

    pub fn child(&self) -> Self {
        Self {
            depth: self.depth + 1,
            parent: Some(Box::new(self.clone())),
            variables: HashSet::new(),
        }
    }
}

/// Data flow tracker for sensitive information.
pub struct DataFlowTracker {
    sources: Vec<SourcePattern>,
    sinks: Vec<SinkPattern>,
    tracked_vars: HashMap<String, DataFlowNode>,
}

#[derive(Debug, Clone)]
pub struct SourcePattern {
    pub pattern: Regex,
    pub description: String,
    pub risk_level: SecuritySeverity,
}

#[derive(Debug, Clone)]
pub struct SinkPattern {
    pub pattern: Regex,
    pub description: String,
    pub risk_level: SecuritySeverity,
}

#[derive(Debug, Clone)]
pub struct DataFlowNode {
    pub var_name: String,
    pub line: usize,
    pub source_type: String,
    pub sanitized: bool,
}

impl Default for DataFlowTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl DataFlowTracker {
    pub fn new() -> Self {
        Self {
            sources: Self::default_sources(),
            sinks: Self::default_sinks(),
            tracked_vars: HashMap::new(),
        }
    }

    fn default_sources() -> Vec<SourcePattern> {
        vec![
            SourcePattern {
                pattern: Regex::new(r"(?i)(getParameter|getQueryParameter|request\.param|req\.body|req\.query|input\()").expect("unwrap failed: security_scanner.rs:192"),
                description: "User input source".to_string(),
                risk_level: SecuritySeverity::High,
            },
            SourcePattern {
                pattern: Regex::new(r"(?i)(stdin|readline|input\(|gets\()").expect("unwrap failed: security_scanner.rs:197"),
                description: "Standard input source".to_string(),
                risk_level: SecuritySeverity::High,
            },
            SourcePattern {
                pattern: Regex::new(r"(?i)(file_get_contents|fopen|readfile|fs\.readFile)").expect("unwrap failed: security_scanner.rs:202"),
                description: "File input source".to_string(),
                risk_level: SecuritySeverity::Medium,
            },
        ]
    }

    fn default_sinks() -> Vec<SinkPattern> {
        vec![
            SinkPattern {
                pattern: Regex::new(r"(?i)(exec|system|popen|shell_exec|spawn)\s*\(").expect("unwrap failed: security_scanner.rs:212"),
                description: "Command execution sink".to_string(),
                risk_level: SecuritySeverity::Critical,
            },
            SinkPattern {
                pattern: Regex::new(r"(?i)(query|execute|exec\s*\()\s*\(.*\+").expect("unwrap failed: security_scanner.rs:217"),
                description: "SQL execution sink".to_string(),
                risk_level: SecuritySeverity::Critical,
            },
            SinkPattern {
                pattern: Regex::new(r"(?i)(innerHTML|dangerouslySetInnerHTML|document\.write)").expect("unwrap failed: security_scanner.rs:222"),
                description: "DOM manipulation sink".to_string(),
                risk_level: SecuritySeverity::High,
            },
            SinkPattern {
                pattern: Regex::new(r"(?i)(eval|Function\()").expect("unwrap failed: security_scanner.rs:227"),
                description: "Code execution sink".to_string(),
                risk_level: SecuritySeverity::Critical,
            },
        ]
    }

    pub fn analyze_data_flow(&mut self, content: &str) -> Vec<SecurityFinding> {
        let mut findings = Vec::new();

        for line in content.lines().enumerate() {
            let line_num = line.0 + 1;
            let line_text = line.1;

            // Check for sources
            for source in &self.sources {
                if source.pattern.is_match(line_text) {
                    // Extract variable names
                    if let Some(var_match) = Regex::new(r"(\w+)\s*=").expect("unwrap failed: security_scanner.rs:245").find(line_text) {
                        let var_name = var_match.as_str().trim_end_matches('=').trim().to_string();
                        self.tracked_vars.insert(var_name.clone(), DataFlowNode {
                            var_name: var_name.clone(),
                            line: line_num,
                            source_type: source.description.clone(),
                            sanitized: false,
                        });
                    }
                }
            }

            // Check for sinks
            for sink in &self.sinks {
                if sink.pattern.is_match(line_text) {
                    // Check if any tracked variable is used unsanitized
                    for (var_name, node) in &self.tracked_vars {
                        if line_text.contains(var_name) && !node.sanitized {
                            findings.push(SecurityFinding {
                                id: format!("FLOW_{}_{}", sink.description.replace(' ', "_").to_uppercase(), line_num),
                                severity: sink.risk_level,
                                category: SecurityCategory::Injection,
                                title: format!("Potential data flow to {}", sink.description),
                                description: format!(
                                    "User-controlled data from '{}' reaches sink without sanitization",
                                    node.source_type
                                ),
                                file: String::new(),
                                line: line_num,
                                code_snippet: line_text.to_string(),
                                recommendation: format!(
                                    "Sanitize data from {} before using in {}",
                                    node.source_type, sink.description
                                ),
                                cwe_id: Some("CWE-20".to_string()),
                            });
                        }
                    }
                }
            }
        }

        findings
    }
}

/// Security scan configuration.
#[derive(Debug, Clone)]
pub struct SecurityScanConfig {
    pub scan_secrets: bool,
    pub scan_injection: bool,
    pub scan_crypto: bool,
    pub scan_auth: bool,
    pub min_severity: SecuritySeverity,
    pub exclude_patterns: Vec<String>,
}

impl Default for SecurityScanConfig {
    fn default() -> Self {
        Self {
            scan_secrets: true,
            scan_injection: true,
            scan_crypto: true,
            scan_auth: true,
            min_severity: SecuritySeverity::Low,
            exclude_patterns: vec![
                r"node_modules".to_string(),
                r"\.min\.js$".to_string(),
                r"vendor/".to_string(),
            ],
        }
    }
}

/// Secret pattern.
#[derive(Debug, Clone)]
pub struct SecretPattern {
    pub name: String,
    pub pattern: Regex,
    pub severity: SecuritySeverity,
    pub description: String,
}

/// Security scanner.
pub struct SecurityScanner {
    config: SecurityScanConfig,
    secret_patterns: Vec<SecretPattern>,
}

impl SecurityScanner {
    pub fn new(config: SecurityScanConfig) -> Self {
        Self {
            config,
            secret_patterns: Self::default_secret_patterns(),
        }
    }

    /// Default secret detection patterns.
    fn default_secret_patterns() -> Vec<SecretPattern> {
        vec![
            SecretPattern {
                name: "AWS Access Key".to_string(),
                pattern: Regex::new(r"(?i)(aws_access_key|aws_secret_key|amazon_wsdl_key)").expect("unwrap failed: security_scanner.rs:347"),
                severity: SecuritySeverity::Critical,
                description: "AWS credentials detected in code".to_string(),
            },
            SecretPattern {
                name: "Generic API Key".to_string(),
                pattern: Regex::new(r"(?i)(api_key|apikey|api-key)").expect("unwrap failed: security_scanner.rs:353"),
                severity: SecuritySeverity::High,
                description: "API key detected in code".to_string(),
            },
            SecretPattern {
                name: "Private Key".to_string(),
                pattern: Regex::new(r"-----BEGIN (RSA |EC |DSA )?PRIVATE KEY-----").expect("unwrap failed: security_scanner.rs:359"),
                severity: SecuritySeverity::Critical,
                description: "Private key detected in code".to_string(),
            },
            SecretPattern {
                name: "Generic Secret".to_string(),
                pattern: Regex::new(r"(?i)(secret|password|passwd|pwd)").expect("unwrap failed: security_scanner.rs:365"),
                severity: SecuritySeverity::High,
                description: "Potential password or secret detected".to_string(),
            },
            SecretPattern {
                name: "JWT Token".to_string(),
                pattern: Regex::new(r"eyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}").expect("invalid regex: security_scanner.rs:371"),
                severity: SecuritySeverity::High,
                description: "JWT token detected".to_string(),
            },
            SecretPattern {
                name: "GitHub Token".to_string(),
                pattern: Regex::new(r"gh[pousr]_[A-Za-z0-9]{36,}").expect("invalid regex: security_scanner.rs:377"),
                severity: SecuritySeverity::Critical,
                description: "GitHub token detected".to_string(),
            },
            SecretPattern {
                name: "Slack Token".to_string(),
                pattern: Regex::new(r"xox[baprs]-[0-9]{10,13}-[0-9]{10,13}[a-zA-Z0-9-]*").expect("invalid regex: security_scanner.rs:383"),
                severity: SecuritySeverity::High,
                description: "Slack token detected".to_string(),
            },
            SecretPattern {
                name: "Database URL".to_string(),
                pattern: Regex::new(r"(?i)(mysql|postgres|postgres|redis|mongodb)://").expect("unwrap failed: security_scanner.rs:389"),
                severity: SecuritySeverity::High,
                description: "Database connection string with credentials".to_string(),
            },
        ]
    }

    /// Scan a file for security issues.
    pub fn scan_file(&self, file_path: &str, content: &str) -> Vec<SecurityFinding> {
        let mut findings = Vec::new();

        // Check exclusions
        for pattern in &self.config.exclude_patterns {
            if Regex::new(pattern).map(|r| r.is_match(file_path)).unwrap_or(false) {
                return findings;
            }
        }

        // Scan for secrets
        if self.config.scan_secrets {
            findings.extend(self.scan_secrets(file_path, content));
        }

        // Scan for injection vulnerabilities
        if self.config.scan_injection {
            findings.extend(self.scan_injection(file_path, content));
        }

        // Scan for crypto issues
        if self.config.scan_crypto {
            findings.extend(self.scan_crypto(file_path, content));
        }

        // Scan for auth issues
        if self.config.scan_auth {
            findings.extend(self.scan_auth(file_path, content));
        }

        // Advanced semantic analysis
        findings.extend(self.scan_advanced_vulnerabilities(file_path, content));

        // Data flow analysis
        let mut tracker = DataFlowTracker::new();
        findings.extend(tracker.analyze_data_flow(content).into_iter().map(|mut f| {
            f.file = file_path.to_string();
            f
        }));

        // Filter by minimum severity
        findings.retain(|f| f.severity <= self.config.min_severity);

        findings
    }

    /// Scan for advanced vulnerabilities using semantic analysis.
    fn scan_advanced_vulnerabilities(&self, file: &str, content: &str) -> Vec<SecurityFinding> {
        let mut findings = Vec::new();

        findings.extend(self.scan_insecure_deserialization(file, content));
        findings.extend(self.scan_xxe(file, content));
        findings.extend(self.scan_cors(file, content));
        findings.extend(self.scan_ssrf(file, content));
        findings.extend(self.scan_race_condition(file, content));
        findings.extend(self.scan_memory_safety(file, content));

        findings
    }

    /// Scan for insecure deserialization vulnerabilities.
    fn scan_insecure_deserialization(&self, file: &str, content: &str) -> Vec<SecurityFinding> {
        let mut findings = Vec::new();

        let patterns = [
            (
                "pickle.loads",
                Regex::new(r"pickle\.loads\s*\(").expect("invalid regex: security_scanner.rs:464"),
                SecuritySeverity::Critical,
                "CWE-502",
            ),
            (
                "yaml.load without safe loader",
                Regex::new(r"yaml\.load\s*\([^)]+\)").expect("unwrap failed: security_scanner.rs:470"),
                SecuritySeverity::High,
                "CWE-20",
            ),
            (
                "XMLDecoder",
                Regex::new(r"XMLDecoder").expect("invalid regex: security_scanner.rs:476"),
                SecuritySeverity::High,
                "CWE-91",
            ),
            (
                "ObjectInputStream",
                Regex::new(r"ObjectInputStream").expect("invalid regex: security_scanner.rs:482"),
                SecuritySeverity::High,
                "CWE-502",
            ),
            (
                "json.loads with eval",
                Regex::new(r"(?i)eval\s*\(.*json\.loads").expect("unwrap failed: security_scanner.rs:488"),
                SecuritySeverity::Critical,
                "CWE-95",
            ),
        ];

        for (name, pattern, severity, cwe) in patterns {
            for mat in pattern.find_iter(content) {
                findings.push(SecurityFinding {
                    id: format!("DESERIAL_{}_{}", name.replace(' ', "_").to_uppercase(), self.get_line_number(content, mat.start())),
                    severity,
                    category: SecurityCategory::InsecureDeserialization,
                    title: name.to_string(),
                    description: "Insecure deserialization can lead to remote code execution".to_string(),
                    file: file.to_string(),
                    line: self.get_line_number(content, mat.start()),
                    code_snippet: self.get_snippet(content, mat.start(), 40),
                    recommendation: "Use safe deserialization methods or validate input thoroughly".to_string(),
                    cwe_id: Some(cwe.to_string()),
                });
            }
        }

        findings
    }

    /// Scan for XXE vulnerabilities.
    fn scan_xxe(&self, file: &str, content: &str) -> Vec<SecurityFinding> {
        let mut findings = Vec::new();

        let patterns = [
            (
                "DTD Processing Enabled",
                Regex::new(r"(?i)(dtdProcessing|feature_disallow_doctype_decl|external-general-entities)").expect("unwrap failed: security_scanner.rs:521"),
                SecuritySeverity::High,
                "CWE-611",
            ),
            (
                "XMLInputFactory XXE",
                Regex::new(r"XMLInputFactory\.newInstance").expect("invalid regex: security_scanner.rs:527"),
                SecuritySeverity::High,
                "CWE-611",
            ),
            (
                "NoInputValidation",
                Regex::new(r"(?i)SAXParser.*without.*validation").expect("unwrap failed: security_scanner.rs:533"),
                SecuritySeverity::Medium,
                "CWE-20",
            ),
        ];

        for (name, pattern, severity, cwe) in patterns {
            for mat in pattern.find_iter(content) {
                findings.push(SecurityFinding {
                    id: format!("XXE_{}_{}", name.replace(' ', "_").to_uppercase(), self.get_line_number(content, mat.start())),
                    severity,
                    category: SecurityCategory::XXE,
                    title: "XML External Entity (XXE) Vulnerability".to_string(),
                    description: format!("Potential XXE vulnerability: {}", name).to_string(),
                    file: file.to_string(),
                    line: self.get_line_number(content, mat.start()),
                    code_snippet: self.get_snippet(content, mat.start(), 50),
                    recommendation: "Disable DTD processing and external entity resolution".to_string(),
                    cwe_id: Some(cwe.to_string()),
                });
            }
        }

        findings
    }

    /// Scan for CORS misconfigurations.
    fn scan_cors(&self, file: &str, content: &str) -> Vec<SecurityFinding> {
        let mut findings = Vec::new();

        let patterns = [
            (
                "CORS Allow All",
                Regex::new(r"(?i)(Access-Control-Allow-Origin.*\*|allow_origin.*\*|cors.*origins.*\*|cors.*credentials.*true)").expect("unwrap failed: security_scanner.rs:566"),
                SecuritySeverity::High,
                "CWE-942",
            ),
            (
                "Wildcard CORS",
                Regex::new(r#"(?i)(setHeader.*Access-Control-Allow-Origin.*\*|res\.header.*\*|Response\.Headers.*Add.*\*|@CrossOrigin.*origins.*=.*"\*")"#).expect("unwrap failed: security_scanner.rs:572"),
                SecuritySeverity::High,
                "CWE-942",
            ),
        ];

        for (name, pattern, severity, cwe) in patterns {
            for mat in pattern.find_iter(content) {
                findings.push(SecurityFinding {
                    id: format!("CORS_{}_{}", name.replace(' ', "_").to_uppercase(), self.get_line_number(content, mat.start())),
                    severity,
                    category: SecurityCategory::Cors,
                    title: "CORS Misconfiguration".to_string(),
                    description: "CORS policy allows all origins, which can lead to unauthorized access".to_string(),
                    file: file.to_string(),
                    line: self.get_line_number(content, mat.start()),
                    code_snippet: self.get_snippet(content, mat.start(), 60),
                    recommendation: "Specify exact allowed origins instead of using wildcard".to_string(),
                    cwe_id: Some(cwe.to_string()),
                });
            }
        }

        findings
    }

    /// Scan for SSRF vulnerabilities.
    fn scan_ssrf(&self, file: &str, content: &str) -> Vec<SecurityFinding> {
        let mut findings = Vec::new();

        let patterns = [
            (
                "URL Fetch Without Validation",
                Regex::new(r"(?i)(requests\.get|urllib\.request|http\.get|fetch|urlopen).*\(").expect("unwrap failed: security_scanner.rs:605"),
                SecuritySeverity::High,
                "CWE-918",
            ),
            (
                "Open Redirect to URL",
                Regex::new(r"(?i)(redirect|forward|location)\s*.*\+.*(url|request|param)").expect("unwrap failed: security_scanner.rs:611"),
                SecuritySeverity::Medium,
                "CWE-601",
            ),
            (
                "File URL Access",
                Regex::new(r"(?i)(file://|phar://|zip://|dict://|gopher://)").expect("unwrap failed: security_scanner.rs:617"),
                SecuritySeverity::High,
                "CWE-918",
            ),
        ];

        for (name, pattern, severity, cwe) in patterns {
            for mat in pattern.find_iter(content) {
                findings.push(SecurityFinding {
                    id: format!("SSRF_{}_{}", name.replace(' ', "_").to_uppercase(), self.get_line_number(content, mat.start())),
                    severity,
                    category: SecurityCategory::SSRF,
                    title: "Server-Side Request Forgery (SSRF)".to_string(),
                    description: format!("Potential SSRF: {}", name).to_string(),
                    file: file.to_string(),
                    line: self.get_line_number(content, mat.start()),
                    code_snippet: self.get_snippet(content, mat.start(), 50),
                    recommendation: "Validate and sanitize URL parameters, use allowlists".to_string(),
                    cwe_id: Some(cwe.to_string()),
                });
            }
        }

        findings
    }

    /// Scan for race condition vulnerabilities.
    fn scan_race_condition(&self, file: &str, content: &str) -> Vec<SecurityFinding> {
        let mut findings = Vec::new();

        let patterns = [
            (
                "Time-of-Check-Time-of-Use",
                Regex::new(r"(?i)(file_exists|access|stat).*&&.*(unlink|mkdir|chmod|chown)").expect("unwrap failed: security_scanner.rs:650"),
                SecuritySeverity::Medium,
                "CWE-367",
            ),
            (
                "Shared Resource Access",
                Regex::new(r"(?i)(lock|mutex|semaphore).*without.*(lock|wait)").expect("unwrap failed: security_scanner.rs:656"),
                SecuritySeverity::Medium,
                "CWE-362",
            ),
        ];

        for (name, pattern, severity, cwe) in patterns {
            for mat in pattern.find_iter(content) {
                findings.push(SecurityFinding {
                    id: format!("RACE_{}_{}", name.replace(' ', "_").to_uppercase(), self.get_line_number(content, mat.start())),
                    severity,
                    category: SecurityCategory::RaceCondition,
                    title: "Race Condition Vulnerability".to_string(),
                    description: format!("Potential race condition: {}", name).to_string(),
                    file: file.to_string(),
                    line: self.get_line_number(content, mat.start()),
                    code_snippet: self.get_snippet(content, mat.start(), 60),
                    recommendation: "Use atomic operations or proper synchronization".to_string(),
                    cwe_id: Some(cwe.to_string()),
                });
            }
        }

        findings
    }

    /// Scan for memory safety issues.
    fn scan_memory_safety(&self, file: &str, content: &str) -> Vec<SecurityFinding> {
        let mut findings = Vec::new();

        let patterns = [
            (
                "Buffer Overflow Risk",
                Regex::new(r"(?i)(strcpy|sprintf|gets|scanf.*%s)").expect("unwrap failed: security_scanner.rs:689"),
                SecuritySeverity::High,
                "CWE-120",
            ),
            (
                "Use After Free",
                Regex::new(r"(?i)(free.*delete|delete.*free)").expect("unwrap failed: security_scanner.rs:695"),
                SecuritySeverity::Critical,
                "CWE-416",
            ),
            (
                "Null Pointer Dereference",
                Regex::new(r"(?i)(\*.*==\s*NULL|->.*==\s*nullptr)").expect("unwrap failed: security_scanner.rs:701"),
                SecuritySeverity::Medium,
                "CWE-476",
            ),
            (
                "Integer Overflow",
                Regex::new(r"(?i)(malloc|alloc|calloc).*\([^)]*\*").expect("unwrap failed: security_scanner.rs:707"),
                SecuritySeverity::High,
                "CWE-190",
            ),
        ];

        for (name, pattern, severity, cwe) in patterns {
            for mat in pattern.find_iter(content) {
                findings.push(SecurityFinding {
                    id: format!("MEM_{}_{}", name.replace(' ', "_").to_uppercase(), self.get_line_number(content, mat.start())),
                    severity,
                    category: SecurityCategory::MemorySafety,
                    title: "Memory Safety Issue".to_string(),
                    description: format!("Potential memory safety issue: {}", name).to_string(),
                    file: file.to_string(),
                    line: self.get_line_number(content, mat.start()),
                    code_snippet: self.get_snippet(content, mat.start(), 40),
                    recommendation: "Use safe alternatives and proper memory management".to_string(),
                    cwe_id: Some(cwe.to_string()),
                });
            }
        }

        findings
    }

    /// Scan for secrets.
    fn scan_secrets(&self, file: &str, content: &str) -> Vec<SecurityFinding> {
        let mut findings = Vec::new();

        for pattern in &self.secret_patterns {
            if let Some(matches) = pattern.pattern.find(content) {
                findings.push(SecurityFinding {
                    id: format!("SECRET_{}_{}", pattern.name.replace(' ', "_"), self.get_line_number(content, matches.start())),
                    severity: pattern.severity,
                    category: SecurityCategory::Secret,
                    title: pattern.name.clone(),
                    description: pattern.description.clone(),
                    file: file.to_string(),
                    line: self.get_line_number(content, matches.start()),
                    code_snippet: self.get_snippet(content, matches.start(), 40),
                    recommendation: "Remove hardcoded secrets and use environment variables or a secrets manager".to_string(),
                    cwe_id: Some("CWE-798".to_string()),
                });
            }
        }

        findings
    }

    /// Scan for injection vulnerabilities.
    fn scan_injection(&self, file: &str, content: &str) -> Vec<SecurityFinding> {
        let mut findings = Vec::new();

        let injection_patterns = [
            (
                "SQL Injection",
                Regex::new(r#"(?i)(execute|query|select|insert|update|delete).*\+.*['\"]"#).expect("unwrap failed: security_scanner.rs:764"),
                SecuritySeverity::Critical,
                "Potential SQL injection vulnerability",
                "CWE-89",
            ),
            (
                "Command Injection",
                Regex::new(r#"(?i)(exec|system|popen|spawn|eval)\s*\("#).expect("unwrap failed: security_scanner.rs:771"),
                SecuritySeverity::Critical,
                "Potential command injection vulnerability",
                "CWE-78",
            ),
            (
                "Path Traversal",
                Regex::new(r#"(?i)(open|read|file_get_contents).*\.\.\/"#).expect("unwrap failed: security_scanner.rs:778"),
                SecuritySeverity::High,
                "Potential path traversal vulnerability",
                "CWE-22",
            ),
            (
                "XSS",
                Regex::new(r"(?i)(innerHTML|dangerouslySetInnerHTML|document\.write)\s*\(").expect("unwrap failed: security_scanner.rs:785"),
                SecuritySeverity::High,
                "Potential XSS vulnerability",
                "CWE-79",
            ),
            (
                "Eval Usage",
                Regex::new(r"(?i)\beval\s*\(").expect("unwrap failed: security_scanner.rs:792"),
                SecuritySeverity::Medium,
                "Use of eval() is a security risk",
                "CWE-95",
            ),
        ];

        for (name, pattern, severity, desc, cwe) in injection_patterns {
            for mat in pattern.find_iter(content) {
                findings.push(SecurityFinding {
                    id: format!("INJECT_{}_{}", name.replace(' ', "_"), self.get_line_number(content, mat.start())),
                    severity,
                    category: SecurityCategory::Injection,
                    title: name.to_string(),
                    description: desc.to_string(),
                    file: file.to_string(),
                    line: self.get_line_number(content, mat.start()),
                    code_snippet: self.get_snippet(content, mat.start(), 30),
                    recommendation: format!("Validate and sanitize user input before using in {}", name),
                    cwe_id: Some(cwe.to_string()),
                });
            }
        }

        findings
    }

    /// Scan for cryptography issues.
    fn scan_crypto(&self, file: &str, content: &str) -> Vec<SecurityFinding> {
        let mut findings = Vec::new();

        let crypto_patterns = [
            (
                "Weak Crypto",
                Regex::new(r"(?i)(md5|sha1|des|rc4)").expect("unwrap failed: security_scanner.rs:826"),
                SecuritySeverity::High,
                "Weak cryptographic algorithm detected",
                "CWE-327",
            ),
            (
                "Hardcoded IV",
                Regex::new(r"(?i)(iv|initialization_vector)").expect("unwrap failed: security_scanner.rs:833"),
                SecuritySeverity::Medium,
                "Hardcoded initialization vector",
                "CWE-329",
            ),
            (
                "ECB Mode",
                Regex::new(r"(?i)ECB").expect("unwrap failed: security_scanner.rs:840"),
                SecuritySeverity::High,
                "ECB mode is not secure for encryption",
                "CWE-256",
            ),
        ];

        for (name, pattern, severity, desc, cwe) in crypto_patterns {
            for mat in pattern.find_iter(content) {
                findings.push(SecurityFinding {
                    id: format!("CRYPTO_{}_{}", name.replace(' ', "_"), self.get_line_number(content, mat.start())),
                    severity,
                    category: SecurityCategory::Cryptography,
                    title: name.to_string(),
                    description: desc.to_string(),
                    file: file.to_string(),
                    line: self.get_line_number(content, mat.start()),
                    code_snippet: self.get_snippet(content, mat.start(), 30),
                    recommendation: "Use strong cryptographic algorithms (AES-256, SHA-256)".to_string(),
                    cwe_id: Some(cwe.to_string()),
                });
            }
        }

        findings
    }

    /// Scan for authentication issues.
    fn scan_auth(&self, file: &str, content: &str) -> Vec<SecurityFinding> {
        let mut findings = Vec::new();

        let auth_patterns = [
            (
                "Hardcoded Credentials",
                Regex::new(r"(?i)(username|user)").expect("unwrap failed: security_scanner.rs:874"),
                SecuritySeverity::High,
                "Hardcoded username detected",
                "CWE-798",
            ),
            (
                "Weak Password",
                Regex::new(r"(?i)(password|pwd)").expect("unwrap failed: security_scanner.rs:881"),
                SecuritySeverity::Medium,
                "Weak password detected",
                "CWE-521",
            ),
            (
                "SQL Auth Query",
                Regex::new(r"(?i)(select|insert).*from.*users.*where.*password").expect("unwrap failed: security_scanner.rs:888"),
                SecuritySeverity::Critical,
                "Plaintext password comparison in SQL",
                "CWE-287",
            ),
        ];

        for (name, pattern, severity, desc, cwe) in auth_patterns {
            for mat in pattern.find_iter(content) {
                findings.push(SecurityFinding {
                    id: format!("AUTH_{}_{}", name.replace(' ', "_"), self.get_line_number(content, mat.start())),
                    severity,
                    category: SecurityCategory::Authentication,
                    title: name.to_string(),
                    description: desc.to_string(),
                    file: file.to_string(),
                    line: self.get_line_number(content, mat.start()),
                    code_snippet: self.get_snippet(content, mat.start(), 40),
                    recommendation: "Use secure authentication mechanisms".to_string(),
                    cwe_id: Some(cwe.to_string()),
                });
            }
        }

        findings
    }

    /// Get line number from byte offset.
    fn get_line_number(&self, content: &str, byte_offset: usize) -> usize {
        content[..byte_offset].matches('\n').count() + 1
    }

    /// Get code snippet around match.
    fn get_snippet(&self, content: &str, byte_offset: usize, context: usize) -> String {
        let start = byte_offset.saturating_sub(context);
        let end = (byte_offset + context).min(content.len());

        let snippet = &content[start..end];
        snippet.lines().take(3).collect::<Vec<_>>().join("\n")
    }

    /// Generate security report.
    pub fn generate_report(&self, findings: &[SecurityFinding]) -> SecurityReport {
        let mut severity_counts = HashMap::new();
        let mut category_counts = HashMap::new();

        for finding in findings {
            *severity_counts.entry(finding.severity).or_insert(0) += 1;
            *category_counts.entry(finding.category).or_insert(0) += 1;
        }

        let risk_score = self.calculate_risk_score(&severity_counts);

        SecurityReport {
            total_findings: findings.len(),
            severity_counts,
            category_counts,
            risk_score,
            top_findings: findings.iter().take(10).cloned().collect(),
        }
    }

    /// Calculate overall risk score.
    fn calculate_risk_score(&self, severity_counts: &HashMap<SecuritySeverity, usize>) -> f32 {
        let weights = [
            (SecuritySeverity::Critical, 10.0),
            (SecuritySeverity::High, 7.5),
            (SecuritySeverity::Medium, 5.0),
            (SecuritySeverity::Low, 2.5),
            (SecuritySeverity::Info, 1.0),
        ];

        let mut score = 0.0;
        for (severity, weight) in weights {
            if let Some(&count) = severity_counts.get(&severity) {
                score += weight * count as f32;
            }
        }

        // Normalize to 0-100
        (score / 10.0).min(100.0)
    }
}

impl Default for SecurityScanner {
    fn default() -> Self {
        Self::new(SecurityScanConfig::default())
    }
}

#[derive(Debug, Clone)]
pub struct SecurityReport {
    pub total_findings: usize,
    pub severity_counts: HashMap<SecuritySeverity, usize>,
    pub category_counts: HashMap<SecurityCategory, usize>,
    pub risk_score: f32,
    pub top_findings: Vec<SecurityFinding>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_aws_key() {
        let scanner = SecurityScanner::default();
        let findings = scanner.scan_file(
            "test.rs",
            "let aws_key = \"AKIAIOSFODNN7EXAMPLE\";",
        );

        assert!(!findings.is_empty());
        assert_eq!(findings[0].severity, SecuritySeverity::Critical);
    }

    #[test]
    fn test_detect_sql_injection() {
        let scanner = SecurityScanner::default();
        let findings = scanner.scan_file(
            "test.py",
            "query = \"SELECT * FROM users WHERE id=\" + user_id",
        );

        assert!(!findings.is_empty());
        assert_eq!(findings[0].category, SecurityCategory::Injection);
    }

    #[test]
    fn test_detect_weak_crypto() {
        let scanner = SecurityScanner::default();
        let findings = scanner.scan_file(
            "test.js",
            "const hash = md5(password);",
        );

        assert!(!findings.is_empty());
        assert_eq!(findings[0].category, SecurityCategory::Cryptography);
    }

    #[test]
    fn test_report_generation() {
        let scanner = SecurityScanner::default();
        let findings = scanner.scan_file(
            "test.rs",
            "let password = \"secret123\";",
        );

        let report = scanner.generate_report(&findings);
        assert_eq!(report.total_findings, findings.len());
        assert!(report.risk_score > 0.0);
    }
}
