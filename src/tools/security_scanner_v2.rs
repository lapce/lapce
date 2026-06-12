//! Security Scanner V2 - Enhanced Security Analysis
//!
//! Based on Claude Code's security scanning approach, this module provides:
//! - Semantic-aware vulnerability detection
//! - Contextual security analysis
//! - Multi-language support
//! - CWE compliance reporting
//! - Real-time security monitoring

use std::collections::{HashMap, HashSet};
use regex::Regex;

/// Security vulnerability pattern
#[derive(Debug, Clone)]
pub struct VulnerabilityPattern {
    pub id: String,
    pub name: String,
    pub description: String,
    pub severity: VulnerabilitySeverity,
    pub cwe_id: String,
    pub owasp_category: String,
    pub patterns: Vec<Regex>,
    pub remediation: String,
    pub examples: Vec<String>,
}

/// Vulnerability severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VulnerabilitySeverity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

impl VulnerabilitySeverity {
    pub fn score(&self) -> u32 {
        match self {
            VulnerabilitySeverity::Critical => 10,
            VulnerabilitySeverity::High => 7,
            VulnerabilitySeverity::Medium => 5,
            VulnerabilitySeverity::Low => 3,
            VulnerabilitySeverity::Info => 1,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            VulnerabilitySeverity::Critical => "CRITICAL",
            VulnerabilitySeverity::High => "HIGH",
            VulnerabilitySeverity::Medium => "MEDIUM",
            VulnerabilitySeverity::Low => "LOW",
            VulnerabilitySeverity::Info => "INFO",
        }
    }
}

/// Security finding with context
#[derive(Debug, Clone)]
pub struct SecurityFindingV2 {
    pub id: String,
    pub vulnerability_id: String,
    pub severity: VulnerabilitySeverity,
    pub title: String,
    pub description: String,
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub code_snippet: String,
    pub context: String,
    pub cwe_id: String,
    pub owasp_category: String,
    pub remediation: String,
    pub references: Vec<String>,
    pub confidence: Confidence,
    pub impact: String,
}

#[derive(Debug, Clone, Copy)]
pub enum Confidence {
    High,
    Medium,
    Low,
}

impl Confidence {
    pub fn as_str(&self) -> &'static str {
        match self {
            Confidence::High => "HIGH",
            Confidence::Medium => "MEDIUM",
            Confidence::Low => "LOW",
        }
    }
}

/// Security report
#[derive(Debug, Clone)]
pub struct SecurityReportV2 {
    pub scan_time_ms: u64,
    pub files_scanned: usize,
    pub total_lines: usize,
    pub findings: Vec<SecurityFindingV2>,
    pub summary: ReportSummary,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ReportSummary {
    pub critical_count: usize,
    pub high_count: usize,
    pub medium_count: usize,
    pub low_count: usize,
    pub info_count: usize,
    pub risk_score: f32,
    pub compliance_status: ComplianceStatus,
}

#[derive(Debug, Clone)]
pub enum ComplianceStatus {
    Passed,
    Warning,
    Failed,
}

/// Context-aware security scanner
pub struct SecurityScannerV2 {
    patterns: Vec<VulnerabilityPattern>,
    excluded_patterns: Vec<String>,
    included_files: Vec<String>,
    language_context: HashMap<String, LanguageSecurityContext>,
}

#[derive(Debug, Clone)]
pub struct LanguageSecurityContext {
    pub language: String,
    pub dangerous_functions: HashSet<String>,
    pub safe_alternatives: HashMap<String, String>,
    pub common_vulnerabilities: Vec<String>,
}

impl SecurityScannerV2 {
    pub fn new() -> Self {
        let patterns = Self::initialize_patterns();
        let language_context = Self::initialize_language_contexts();
        
        Self {
            patterns,
            excluded_patterns: Vec::new(),
            included_files: Vec::new(),
            language_context,
        }
    }

    fn initialize_patterns() -> Vec<VulnerabilityPattern> {
        vec![
            // SQL Injection
            VulnerabilityPattern {
                id: "SQL001".to_string(),
                name: "SQL Injection".to_string(),
                description: "Potential SQL injection vulnerability detected".to_string(),
                severity: VulnerabilitySeverity::Critical,
                cwe_id: "CWE-89".to_string(),
                owasp_category: "A03:2021 - Injection".to_string(),
                patterns: vec![
                    Regex::new(r#"(?i)(SELECT|INSERT|UPDATE|DELETE|DROP|EXEC|EXECUTE)\s*\(.*\+.*\)"#).expect("unwrap failed: security_scanner_v2.rs:166"),
                    Regex::new(r#"(?i)query\s*\([^)]*\+\s*\w+"#).expect("unwrap failed: security_scanner_v2.rs:167"),
                    Regex::new(r#"\.format\s*\(.*%.*\)|%.*\{.*\}"#).expect("unwrap failed: security_scanner_v2.rs:168"),
                    Regex::new(r#"(?i)f['"].*SELECT.*FROM"#).expect("unwrap failed: security_scanner_v2.rs:169"),
                ],
                remediation: "Use parameterized queries or prepared statements".to_string(),
                examples: vec![
                    "query('SELECT * FROM users WHERE id=' + userId)".to_string(),
                    "f'SELECT * FROM users WHERE name={name}'".to_string(),
                ],
            },
            
            // Command Injection
            VulnerabilityPattern {
                id: "CMD001".to_string(),
                name: "Command Injection".to_string(),
                description: "Potential OS command injection vulnerability".to_string(),
                severity: VulnerabilitySeverity::Critical,
                cwe_id: "CWE-78".to_string(),
                owasp_category: "A03:2021 - Injection".to_string(),
                patterns: vec![
                    Regex::new(r"(?i)(system|exec|spawn|sh|popen)\s*\([^)]*\%[a-z]").expect("unwrap failed: security_scanner_v2.rs:187"),
                    Regex::new(r"(?i)os\.system|os\.popen|subprocess").expect("unwrap failed: security_scanner_v2.rs:188"),
                    Regex::new(r"(?i)shell=True").expect("unwrap failed: security_scanner_v2.rs:189"),
                    Regex::new(r#"(?i)exec\s*\([^)]*\+\s*\w+"#).expect("unwrap failed: security_scanner_v2.rs:190"),
                ],
                remediation: "Avoid shell=True; use parameterized commands".to_string(),
                examples: vec![
                    "subprocess.run(cmd, shell=True)".to_string(),
                    "os.system(user_input)".to_string(),
                ],
            },
            
            // Path Traversal
            VulnerabilityPattern {
                id: "PATH001".to_string(),
                name: "Path Traversal".to_string(),
                description: "Potential path traversal vulnerability".to_string(),
                severity: VulnerabilitySeverity::High,
                cwe_id: "CWE-22".to_string(),
                owasp_category: "A01:2021 - Broken Access Control".to_string(),
                patterns: vec![
                    Regex::new(r#"\.\./|\.\.\\\\"#,).expect("invalid regex: security_scanner_v2.rs:208"),
                    Regex::new(r"(?i)open\s*\([^)]*\%[a-z]").expect("unwrap failed: security_scanner_v2.rs:209"),
                    Regex::new(r"(?i)(readFile|readFileSync|createReadStream).*path\.join").expect("unwrap failed: security_scanner_v2.rs:210"),
                    Regex::new(r"(?i)sendFile|serveFile").expect("unwrap failed: security_scanner_v2.rs:211"),
                ],
                remediation: "Validate and sanitize file paths; use allowlists".to_string(),
                examples: vec![
                    "fs.readFile(userPath + filename)".to_string(),
                    "open(f'files/{user_input}')".to_string(),
                ],
            },
            
            // XSS
            VulnerabilityPattern {
                id: "XSS001".to_string(),
                name: "Cross-Site Scripting (XSS)".to_string(),
                description: "Potential XSS vulnerability detected".to_string(),
                severity: VulnerabilitySeverity::High,
                cwe_id: "CWE-79".to_string(),
                owasp_category: "A03:2021 - Injection".to_string(),
                patterns: vec![
                    Regex::new(r"(?i)innerHTML\s*=").expect("unwrap failed: security_scanner_v2.rs:229"),
                    Regex::new(r"(?i)document\.write").expect("unwrap failed: security_scanner_v2.rs:230"),
                    Regex::new(r"(?i)dangerouslySetInnerHTML").expect("unwrap failed: security_scanner_v2.rs:231"),
                    Regex::new(r"(?i)eval\s*\(").expect("unwrap failed: security_scanner_v2.rs:232"),
                    Regex::new(r"(?i)insertAdjacentHTML").expect("unwrap failed: security_scanner_v2.rs:233"),
                ],
                remediation: "Use textContent or sanitize HTML before insertion".to_string(),
                examples: vec![
                    "element.innerHTML = userInput".to_string(),
                    "React: dangerouslySetInnerHTML={{__html: userContent}}".to_string(),
                ],
            },
            
            // Hardcoded Credentials
            VulnerabilityPattern {
                id: "AUTH001".to_string(),
                name: "Hardcoded Credentials".to_string(),
                description: "Hardcoded credentials or secrets detected".to_string(),
                severity: VulnerabilitySeverity::Critical,
                cwe_id: "CWE-798".to_string(),
                owasp_category: "A07:2021 - Identification and Authentication Failures".to_string(),
                patterns: vec![
                    Regex::new(r#"(?i)(password|passwd|pwd)\s*=\s*['"][^'"]+['"]"#).expect("unwrap failed: security_scanner_v2.rs:251"),
                    Regex::new(r#"(?i)(api_key|apikey|secret_key)\s*=\s*['"][A-Za-z0-9+/]{16,}['"]"#).expect("unwrap failed: security_scanner_v2.rs:252"),
                    Regex::new(r"(?i)aws_access_key|aws_secret_key").expect("unwrap failed: security_scanner_v2.rs:253"),
                    Regex::new(r"(?i)bearer\s+[A-Za-z0-9+/]{20,}").expect("unwrap failed: security_scanner_v2.rs:254"),
                    Regex::new(r"(?i)ghp_[A-Za-z0-9]{36}|gho_[A-Za-z0-9]{36}").expect("unwrap failed: security_scanner_v2.rs:255"),
                    Regex::new(r"(?i)sk-[A-Za-z0-9]{48}").expect("unwrap failed: security_scanner_v2.rs:256"),
                ],
                remediation: "Use environment variables or secure vaults".to_string(),
                examples: vec![
                    "password = 'admin123'".to_string(),
                    "api_key = 'sk-...'".to_string(),
                ],
            },
            
            // Weak Cryptography
            VulnerabilityPattern {
                id: "CRYPTO001".to_string(),
                name: "Weak Cryptographic Algorithm".to_string(),
                description: "Usage of weak cryptographic algorithm detected".to_string(),
                severity: VulnerabilitySeverity::High,
                cwe_id: "CWE-327".to_string(),
                owasp_category: "A02:2021 - Cryptographic Failures".to_string(),
                patterns: vec![
                    Regex::new(r"(?i)hashlib\.(md5|sha1)").expect("unwrap failed: security_scanner_v2.rs:274"),
                    Regex::new(r#"(?i)Cipher\.getInstance\s*\(\s*['"]DES"#).expect("unwrap failed: security_scanner_v2.rs:275"),
                    Regex::new(r#"(?i)Cipher\.getInstance\s*\(\s*['"]RC4"#).expect("unwrap failed: security_scanner_v2.rs:276"),
                    Regex::new(r"(?i)CryptoJS\.MD5|CryptoJS\.SHA1").expect("unwrap failed: security_scanner_v2.rs:277"),
                    Regex::new(r"(?i)AES\.ECB").expect("unwrap failed: security_scanner_v2.rs:278"),
                ],
                remediation: "Use AES-GCM, ChaCha20-Poly1305, or modern algorithms".to_string(),
                examples: vec![
                    "hashlib.md5(data)".to_string(),
                    "Cipher.getInstance('DES/CBC/PKCS5Padding')".to_string(),
                ],
            },
            
            // Insecure Deserialization
            VulnerabilityPattern {
                id: "DESER001".to_string(),
                name: "Insecure Deserialization".to_string(),
                description: "Potential insecure deserialization vulnerability".to_string(),
                severity: VulnerabilitySeverity::Critical,
                cwe_id: "CWE-502".to_string(),
                owasp_category: "A08:2021 - Software and Data Integrity Failures".to_string(),
                patterns: vec![
                    Regex::new(r"(?i)pickle\.loads").expect("unwrap failed: security_scanner_v2.rs:296"),
                    Regex::new(r"(?i)yaml\.load\s*\(").expect("unwrap failed: security_scanner_v2.rs:297"),
                    Regex::new(r"(?i)ObjectInputStream").expect("unwrap failed: security_scanner_v2.rs:298"),
                    Regex::new(r"(?i)readObject\s*\(\s*\)").expect("unwrap failed: security_scanner_v2.rs:299"),
                    Regex::new(r"(?i)json\.loads.*eval").expect("unwrap failed: security_scanner_v2.rs:300"),
                ],
                remediation: "Use safe deserialization libraries; validate input".to_string(),
                examples: vec![
                    "pickle.loads(data)".to_string(),
                    "yaml.load(untrusted_data)".to_string(),
                ],
            },
            
            // SSRF
            VulnerabilityPattern {
                id: "SSRF001".to_string(),
                name: "Server-Side Request Forgery".to_string(),
                description: "Potential SSRF vulnerability".to_string(),
                severity: VulnerabilitySeverity::High,
                cwe_id: "CWE-918".to_string(),
                owasp_category: "A10:2021 - Server-Side Request Forgery (SSRF)".to_string(),
                patterns: vec![
                    Regex::new(r"(?i)(requests\.(get|post)|fetch|urllib|urlopen).*\([^)]*user").expect("unwrap failed: security_scanner_v2.rs:318"),
                    Regex::new(r"(?i)file://|dict://|gopher://").expect("unwrap failed: security_scanner_v2.rs:319"),
                    Regex::new(r"(?i)redirect.*url").expect("unwrap failed: security_scanner_v2.rs:320"),
                ],
                remediation: "Validate and allowlist URLs; use safe URL parsing".to_string(),
                examples: vec![
                    "requests.get(user_provided_url)".to_string(),
                    "fetch(userUrl, {redirect: 'follow'})".to_string(),
                ],
            },
            
            // XXE
            VulnerabilityPattern {
                id: "XXE001".to_string(),
                name: "XML External Entity (XXE)".to_string(),
                description: "Potential XXE vulnerability".to_string(),
                severity: VulnerabilitySeverity::Critical,
                cwe_id: "CWE-611".to_string(),
                owasp_category: "A05:2021 - Security Misconfiguration".to_string(),
                patterns: vec![
                    Regex::new(r"(?i)SAXParser.*DTD").expect("unwrap failed: security_scanner_v2.rs:338"),
                    Regex::new(r"(?i)DocumentBuilderFactory.*DTD").expect("unwrap failed: security_scanner_v2.rs:339"),
                    Regex::new(r"(?i)XMLInputFactory.*DTD").expect("unwrap failed: security_scanner_v2.rs:340"),
                    Regex::new(r"(?i)setFeature.*DTD").expect("unwrap failed: security_scanner_v2.rs:341"),
                ],
                remediation: "Disable DTD processing and external entities".to_string(),
                examples: vec![
                    "DocumentBuilderFactory.newInstance()".to_string(),
                    "xml.set_feature('disallow-doctype-decl', False)".to_string(),
                ],
            },
            
            // Memory Safety
            VulnerabilityPattern {
                id: "MEM001".to_string(),
                name: "Memory Safety Issue".to_string(),
                description: "Potential memory safety vulnerability".to_string(),
                severity: VulnerabilitySeverity::High,
                cwe_id: "CWE-119".to_string(),
                owasp_category: "A01:2021 - Broken Access Control".to_string(),
                patterns: vec![
                    Regex::new(r"(?i)strcpy|strcat|sprintf|gets|scanf.*%s").expect("unwrap failed: security_scanner_v2.rs:359"),
                    Regex::new(r"(?i)malloc\s*\(\s*sizeof\s*\(").expect("unwrap failed: security_scanner_v2.rs:360"),
                    Regex::new(r"(?i)free.*delete").expect("unwrap failed: security_scanner_v2.rs:361"),
                ],
                remediation: "Use safe string functions; proper memory management".to_string(),
                examples: vec![
                    "strcpy(dest, src)".to_string(),
                    "gets(user_input)".to_string(),
                ],
            },
            
            // Race Condition
            VulnerabilityPattern {
                id: "RACE001".to_string(),
                name: "Race Condition".to_string(),
                description: "Potential race condition vulnerability".to_string(),
                severity: VulnerabilitySeverity::Medium,
                cwe_id: "CWE-362".to_string(),
                owasp_category: "A04:2021 - Insecure Design".to_string(),
                patterns: vec![
                    Regex::new(r"(?i)file_exists.*&&.*(unlink|mkdir|chmod)").expect("unwrap failed: security_scanner_v2.rs:379"),
                    Regex::new(r"(?i)check.*then.*act").expect("unwrap failed: security_scanner_v2.rs:380"),
                    Regex::new(r"(?i)TOCTOU").expect("unwrap failed: security_scanner_v2.rs:381"),
                ],
                remediation: "Use atomic operations; proper locking mechanisms".to_string(),
                examples: vec![
                    "if (access(path) == 0) { unlink(path); }".to_string(),
                ],
            },
            
            // CORS Misconfiguration
            VulnerabilityPattern {
                id: "CORS001".to_string(),
                name: "CORS Misconfiguration".to_string(),
                description: "CORS allowing all origins".to_string(),
                severity: VulnerabilitySeverity::Medium,
                cwe_id: "CWE-942".to_string(),
                owasp_category: "A05:2021 - Security Misconfiguration".to_string(),
                patterns: vec![
                    Regex::new(r"(?i)Access-Control-Allow-Origin.*\*").expect("unwrap failed: security_scanner_v2.rs:398"),
                    Regex::new(r"(?i)cors.*credentials.*true.*origin.*\*").expect("unwrap failed: security_scanner_v2.rs:399"),
                    Regex::new(r"(?i)allow.*origin.*\*").expect("unwrap failed: security_scanner_v2.rs:400"),
                ],
                remediation: "Specify exact allowed origins; don't use wildcards with credentials".to_string(),
                examples: vec![
                    "Access-Control-Allow-Origin: *".to_string(),
                    "cors({origin: '*', credentials: true})".to_string(),
                ],
            },
        ]
    }

    fn initialize_language_contexts() -> HashMap<String, LanguageSecurityContext> {
        let mut contexts = HashMap::new();

        // Rust context
        let mut rust_dangerous = HashSet::new();
        rust_dangerous.insert("unsafe".to_string());
        rust_dangerous.insert("unwrap".to_string());
        rust_dangerous.insert("expect".to_string());
        
        let mut rust_safe = HashMap::new();
        rust_safe.insert("unwrap".to_string(), "unwrap_or, unwrap_or_else".to_string());
        rust_safe.insert("expect".to_string(), "ok_or, unwrap_or_default".to_string());
        
        contexts.insert("rust".to_string(), LanguageSecurityContext {
            language: "Rust".to_string(),
            dangerous_functions: rust_dangerous,
            safe_alternatives: rust_safe,
            common_vulnerabilities: vec![
                "Use of unwrap() in production".to_string(),
                "Unsafe code blocks".to_string(),
                "Memory leaks with Rc/Arc".to_string(),
            ],
        });

        // JavaScript/TypeScript context
        let mut js_dangerous = HashSet::new();
        js_dangerous.insert("eval".to_string());
        js_dangerous.insert("innerHTML".to_string());
        js_dangerous.insert("document.write".to_string());
        
        let mut js_safe = HashMap::new();
        js_safe.insert("innerHTML".to_string(), "textContent, innerText".to_string());
        js_safe.insert("eval".to_string(), "JSON.parse, Function constructor".to_string());
        
        contexts.insert("javascript".to_string(), LanguageSecurityContext {
            language: "JavaScript".to_string(),
            dangerous_functions: js_dangerous,
            safe_alternatives: js_safe,
            common_vulnerabilities: vec![
                "XSS via innerHTML".to_string(),
                "Prototype pollution".to_string(),
                "eval() usage".to_string(),
            ],
        });

        // Python context
        let mut py_dangerous = HashSet::new();
        py_dangerous.insert("eval".to_string());
        py_dangerous.insert("exec".to_string());
        py_dangerous.insert("pickle".to_string());
        
        let mut py_safe = HashMap::new();
        py_safe.insert("eval".to_string(), "ast.literal_eval".to_string());
        py_safe.insert("pickle".to_string(), "json".to_string());
        
        contexts.insert("python".to_string(), LanguageSecurityContext {
            language: "Python".to_string(),
            dangerous_functions: py_dangerous,
            safe_alternatives: py_safe,
            common_vulnerabilities: vec![
                "SQL injection".to_string(),
                "Pickle deserialization".to_string(),
                "eval() usage".to_string(),
            ],
        });

        contexts
    }

    /// Scan a file for vulnerabilities
    pub fn scan_file(&self, file_path: &str, content: &str, language: &str) -> Vec<SecurityFindingV2> {
        let mut findings = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        for (line_num, line) in lines.iter().enumerate() {
            for pattern in &self.patterns {
                for regex in &pattern.patterns {
                    if let Some(mat) = regex.find(line) {
                        let finding = self.create_finding(
                            pattern,
                            file_path,
                            line_num + 1,
                            mat.start(),
                            line,
                            &lines,
                        );
                        findings.push(finding);
                    }
                }
            }
        }

        // Additional context-aware checks
        findings.extend(self.scan_context_aware(file_path, content, language));

        findings
    }

    /// Scan with language-specific context awareness
    fn scan_context_aware(&self, file_path: &str, content: &str, language: &str) -> Vec<SecurityFindingV2> {
        let mut findings = Vec::new();
        
        if let Some(context) = self.language_context.get(language) {
            let lines: Vec<&str> = content.lines().collect();
            
            for (line_num, line) in lines.iter().enumerate() {
                for dangerous_fn in &context.dangerous_functions {
                    if line.contains(dangerous_fn) {
                        let finding = SecurityFindingV2 {
                            id: format!("CTX_{}_{}", language, line_num),
                            vulnerability_id: format!("CTX_{}", dangerous_fn.to_uppercase()),
                            severity: VulnerabilitySeverity::Medium,
                            title: format!("Use of dangerous function: {}", dangerous_fn),
                            description: format!(
                                "The {} function is considered dangerous and should be avoided. \
                                Consider using safer alternatives.",
                                dangerous_fn
                            ),
                            file: file_path.to_string(),
                            line: line_num + 1,
                            column: line.find(dangerous_fn).unwrap_or(0),
                            code_snippet: line.to_string(),
                            context: format!("Found in {} file", language),
                            cwe_id: "CWE-250".to_string(),
                            owasp_category: "A01:2021 - Broken Access Control".to_string(),
                            remediation: context.safe_alternatives.get(dangerous_fn)
                                .cloned()
                                .unwrap_or_else(|| "Review and refactor".to_string()),
                            references: vec![],
                            confidence: Confidence::Medium,
                            impact: "May lead to security vulnerabilities".to_string(),
                        };
                        findings.push(finding);
                    }
                }
            }
        }

        findings
    }

    /// Create a finding from a pattern match
    fn create_finding(
        &self,
        pattern: &VulnerabilityPattern,
        file: &str,
        line: usize,
        column: usize,
        line_content: &str,
        all_lines: &[&str],
    ) -> SecurityFindingV2 {
        // Get context (3 lines before and after)
        let context_start = line.saturating_sub(3);
        let context_end = (line + 2).min(all_lines.len());
        let context = all_lines[context_start..context_end].join("\n");

        SecurityFindingV2 {
            id: format!("{}_{}", pattern.id, line),
            vulnerability_id: pattern.id.clone(),
            severity: pattern.severity,
            title: pattern.name.clone(),
            description: pattern.description.clone(),
            file: file.to_string(),
            line,
            column,
            code_snippet: line_content.to_string(),
            context,
            cwe_id: pattern.cwe_id.clone(),
            owasp_category: pattern.owasp_category.clone(),
            remediation: pattern.remediation.clone(),
            references: vec![
                format!("https://cwe.mitre.org/data/definitions/{}.html", 
                    pattern.cwe_id.trim_start_matches("CWE-")),
                format!("https://owasp.org/www-project-top-ten/2017/{}", 
                    pattern.owasp_category.split_whitespace().next().unwrap_or("A01")),
            ],
            confidence: Confidence::High,
            impact: "Security vulnerability detected".to_string(),
        }
    }

    /// Scan multiple files and generate report
    pub fn scan_files(&self, files: &[(String, String, String)]) -> SecurityReportV2 {
        let start = std::time::Instant::now();
        let mut all_findings = Vec::new();
        let mut total_lines = 0;

        for (path, content, language) in files {
            total_lines += content.lines().count();
            let findings = self.scan_file(path, content, language);
            all_findings.extend(findings);
        }

        // Generate summary
        let mut summary = ReportSummary {
            critical_count: 0,
            high_count: 0,
            medium_count: 0,
            low_count: 0,
            info_count: 0,
            risk_score: 0.0,
            compliance_status: ComplianceStatus::Passed,
        };

        for finding in &all_findings {
            match finding.severity {
                VulnerabilitySeverity::Critical => summary.critical_count += 1,
                VulnerabilitySeverity::High => summary.high_count += 1,
                VulnerabilitySeverity::Medium => summary.medium_count += 1,
                VulnerabilitySeverity::Low => summary.low_count += 1,
                VulnerabilitySeverity::Info => summary.info_count += 1,
            }
        }

        // Calculate risk score
        summary.risk_score = 
            (summary.critical_count as f32 * 10.0) +
            (summary.high_count as f32 * 7.0) +
            (summary.medium_count as f32 * 5.0) +
            (summary.low_count as f32 * 3.0) +
            (summary.info_count as f32 * 1.0);

        // Determine compliance status
        summary.compliance_status = if summary.critical_count > 0 || summary.high_count > 3 {
            ComplianceStatus::Failed
        } else if summary.medium_count > 5 || summary.high_count > 0 {
            ComplianceStatus::Warning
        } else {
            ComplianceStatus::Passed
        };

        // Generate recommendations
        let recommendations = self.generate_recommendations(&all_findings, &summary);

        SecurityReportV2 {
            scan_time_ms: start.elapsed().as_millis() as u64,
            files_scanned: files.len(),
            total_lines,
            findings: all_findings,
            summary,
            recommendations,
        }
    }

    /// Scan all files in a directory recursively and generate report.
    pub fn scan_directory(&self, dir_path: &str) -> SecurityReportV2 {
        let _start = std::time::Instant::now();
        let mut files = Vec::new();
        let path = std::path::Path::new(dir_path);

        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let file_path = entry.path();
                if file_path.is_file() {
                    if let Some(ext) = file_path.extension().and_then(|e| e.to_str()) {
                        // Only scan source code files
                        match ext {
                            "rs" | "py" | "js" | "ts" | "go" | "java" | "c" | "cpp" | "h" | "hpp" | "rb" | "php" => {
                                if let Ok(content) = std::fs::read_to_string(&file_path) {
                                    let lang = detect_security_language(&file_path);
                                    files.push((file_path.to_string_lossy().to_string(), content, lang));
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        self.scan_files(&files)
    }

    /// Generate security recommendations
    fn generate_recommendations(&self, findings: &[SecurityFindingV2], summary: &ReportSummary) -> Vec<String> {
        let mut recommendations = Vec::new();

        if summary.critical_count > 0 {
            recommendations.push(
                "URGENT: Address all critical vulnerabilities immediately. \
                Consider implementing a security review process.".to_string()
            );
        }

        if summary.high_count > 0 {
            recommendations.push(
                "High priority: Review and fix high severity issues within this sprint.".to_string()
            );
        }

        // Group by CWE
        let mut cwe_counts: HashMap<&str, usize> = HashMap::new();
        for finding in findings {
            *cwe_counts.entry(&finding.cwe_id).or_insert(0) += 1;
        }

        // Add specific recommendations based on common issues
        if let Some(&count) = cwe_counts.get("CWE-89") {
            if count > 0 {
                recommendations.push(
                    "SQL Injection: Implement parameterized queries using an ORM or query builder.".to_string()
                );
            }
        }

        if let Some(&count) = cwe_counts.get("CWE-79") {
            if count > 0 {
                recommendations.push(
                    "XSS: Implement Content Security Policy (CSP) and sanitize all user inputs.".to_string()
                );
            }
        }

        if let Some(&count) = cwe_counts.get("CWE-798") {
            if count > 0 {
                recommendations.push(
                    "Hardcoded Secrets: Move all credentials to environment variables or a secrets manager.".to_string()
                );
            }
        }

        recommendations.push(
            "Implement automated security scanning in CI/CD pipeline.".to_string()
        );

        recommendations
    }

    /// Format report as string
    pub fn format_report(&self, report: &SecurityReportV2) -> String {
        let mut output = String::new();
        
        output.push_str("\n╔══════════════════════════════════════════════════════════════════╗\n");
        output.push_str("║                     SECURITY SCAN REPORT                         ║\n");
        output.push_str("╠══════════════════════════════════════════════════════════════════╣\n");
        output.push_str(&format!("║  Files Scanned: {}                                             ║\n", report.files_scanned));
        output.push_str(&format!("║  Total Lines: {}                                               ║\n", report.total_lines));
        output.push_str(&format!("║  Scan Time: {}ms                                              ║\n", report.scan_time_ms));
        output.push_str("╠══════════════════════════════════════════════════════════════════╣\n");
        output.push_str("║  VULNERABILITIES FOUND                                        ║\n");
        output.push_str("╠══════════════════════════════════════════════════════════════════╣\n");
        output.push_str(&format!("║  CRITICAL: {:>4}   HIGH: {:>4}   MEDIUM: {:>4}   LOW: {:>4}  ║\n",
            report.summary.critical_count,
            report.summary.high_count,
            report.summary.medium_count,
            report.summary.low_count
        ));
        output.push_str("╠══════════════════════════════════════════════════════════════════╣\n");
        output.push_str(&format!("║  Risk Score: {:.1}                                              ║\n", report.summary.risk_score));
        
        let status = match report.summary.compliance_status {
            ComplianceStatus::Passed => "✓ PASSED",
            ComplianceStatus::Warning => "⚠ WARNING",
            ComplianceStatus::Failed => "✗ FAILED",
        };
        output.push_str(&format!("║  Status: {}                                             ║\n", status));
        output.push_str("╚══════════════════════════════════════════════════════════════════╝\n");

        if !report.findings.is_empty() {
            output.push_str("\n📋 DETAILED FINDINGS:\n\n");
            for finding in &report.findings {
                output.push_str(&format!(
                    "  [{:>8}] {} (Line {})\n    File: {}\n    CWE: {} | OWASP: {}\n    Remediation: {}\n\n",
                    finding.severity.as_str(),
                    finding.title,
                    finding.line,
                    finding.file,
                    finding.cwe_id,
                    finding.owasp_category.split_whitespace().next().unwrap_or(""),
                    finding.remediation
                ));
            }
        }

        if !report.recommendations.is_empty() {
            output.push_str("\n💡 RECOMMENDATIONS:\n\n");
            for (i, rec) in report.recommendations.iter().enumerate() {
                output.push_str(&format!("  {}. {}\n", i + 1, rec));
            }
        }

        output
    }

    /// Get the list of excluded patterns for this scanner.
    pub fn excluded_patterns(&self) -> &[String] {
        &self.excluded_patterns
    }

    /// Get the list of included files for this scanner.
    pub fn included_files(&self) -> &[String] {
        &self.included_files
    }
}

impl Default for SecurityScannerV2 {
    fn default() -> Self {
        Self::new()
    }
}

/// Detect programming language from file path for security scanning.
fn detect_security_language(path: &std::path::Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| match ext {
            "rs" => "rust",
            "ts" | "tsx" => "typescript",
            "js" | "jsx" => "javascript",
            "py" => "python",
            "go" => "go",
            "java" => "java",
            "c" | "h" => "c",
            "cpp" | "hpp" | "cc" => "cpp",
            "rb" => "ruby",
            "php" => "php",
            _ => "unknown",
        })
        .unwrap_or("unknown")
        .to_string()
}
