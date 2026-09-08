//! Integration Test Generator - Generates integration tests from API definitions.
//!
//! This module provides:
//! - API endpoint detection
//! - Request/response pattern generation
//! - Test fixture creation
//! - Mock setup generation

use std::collections::HashMap;

/// An API endpoint for testing.
#[derive(Debug, Clone)]
pub struct ApiEndpoint {
    /// HTTP method.
    pub method: HttpMethod,
    /// Path pattern.
    pub path: String,
    /// Path parameters.
    pub path_params: Vec<ApiParam>,
    /// Query parameters.
    pub query_params: Vec<ApiParam>,
    /// Request body type.
    pub request_body: Option<String>,
    /// Response body type.
    pub response_body: Option<String>,
    /// Response status codes.
    pub status_codes: Vec<u16>,
    /// Authentication required.
    pub auth_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    GET,
    POST,
    PUT,
    PATCH,
    DELETE,
    OPTIONS,
}

#[derive(Debug, Clone)]
pub struct ApiParam {
    pub name: String,
    pub param_type: String,
    pub required: bool,
    pub description: Option<String>,
}

/// An integration test case.
#[derive(Debug, Clone)]
pub struct IntegrationTest {
    pub id: String,
    pub name: String,
    pub description: String,
    pub endpoints: Vec<String>,
    pub steps: Vec<TestStep>,
    pub fixtures: Vec<Fixture>,
    pub language: TestLanguage,
}

/// A step in an integration test.
#[derive(Debug, Clone)]
pub struct TestStep {
    pub step_number: usize,
    pub action: TestAction,
    pub request: Option<HttpRequest>,
    pub response_check: Option<ResponseCheck>,
    pub setup: Option<String>,
    pub teardown: Option<String>,
}

#[derive(Debug, Clone)]
pub enum TestAction {
    Request,
    Assert,
    Setup,
    Teardown,
    Wait,
}

#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResponseCheck {
    pub status_code: u16,
    pub body_contains: Vec<String>,
    pub json_path_checks: Vec<JsonPathCheck>,
}

#[derive(Debug, Clone)]
pub struct JsonPathCheck {
    pub path: String,
    pub expected_value: Option<String>,
    pub exists: bool,
}

/// A test fixture.
#[derive(Debug, Clone)]
pub struct Fixture {
    pub name: String,
    pub fixture_type: FixtureType,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone)]
pub enum FixtureType {
    User,
    Product,
    Order,
    Database,
    File,
    Mock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestLanguage {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Go,
}

/// Integration test generator.
pub struct IntegrationTestGenerator {
    config: IntegrationTestConfig,
}

#[derive(Debug, Clone)]
pub struct IntegrationTestConfig {
    pub language: TestLanguage,
    pub framework: IntegrationFramework,
    pub include_auth: bool,
    pub include_error_cases: bool,
    pub max_steps: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationFramework {
    RustActix,
    RustAxum,
    PythonFlask,
    PythonFastApi,
    NodeExpress,
    GoGin,
}

impl Default for IntegrationTestConfig {
    fn default() -> Self {
        Self {
            language: TestLanguage::Rust,
            framework: IntegrationFramework::RustActix,
            include_auth: true,
            include_error_cases: true,
            max_steps: 10,
        }
    }
}

impl IntegrationTestGenerator {
    pub fn new(config: IntegrationTestConfig) -> Self {
        Self { config }
    }

    /// Generate integration tests from API endpoints.
    pub fn generate(&self, endpoints: &[ApiEndpoint]) -> Vec<IntegrationTest> {
        let mut tests = Vec::new();

        // Generate happy path tests
        for endpoint in endpoints {
            if let Some(test) = self.generate_happy_path_test(endpoint) {
                tests.push(test);
            }
        }

        // Generate auth tests
        if self.config.include_auth {
            for endpoint in endpoints.iter().filter(|e| e.auth_required) {
                if let Some(test) = self.generate_auth_test(endpoint) {
                    tests.push(test);
                }
            }
        }

        // Generate error case tests
        if self.config.include_error_cases {
            for endpoint in endpoints {
                tests.extend(self.generate_error_tests(endpoint));
            }
        }

        tests
    }

    /// Generate happy path test for an endpoint.
    fn generate_happy_path_test(&self, endpoint: &ApiEndpoint) -> Option<IntegrationTest> {
        let test_name = format!("test_{}_{}", format!("{:?}", endpoint.method).to_lowercase(), self.sanitize_name(&endpoint.path));

        Some(IntegrationTest {
            id: format!("{}_happy", test_name),
            name: test_name.clone(),
            description: format!("Integration test for {} {}", format!("{:?}", endpoint.method), endpoint.path),
            endpoints: vec![endpoint.path.clone()],
            steps: vec![
                TestStep {
                    step_number: 1,
                    action: TestAction::Setup,
                    request: None,
                    response_check: None,
                    setup: Some(self.generate_setup_code(endpoint)),
                    teardown: None,
                },
                TestStep {
                    step_number: 2,
                    action: TestAction::Request,
                    request: Some(self.generate_request(endpoint)),
                    response_check: Some(ResponseCheck {
                        status_code: endpoint.status_codes.first().copied().unwrap_or(200),
                        body_contains: Vec::new(),
                        json_path_checks: Vec::new(),
                    }),
                    setup: None,
                    teardown: None,
                },
            ],
            fixtures: self.generate_fixtures(endpoint),
            language: self.config.language,
        })
    }

    /// Generate authentication test.
    fn generate_auth_test(&self, endpoint: &ApiEndpoint) -> Option<IntegrationTest> {
        Some(IntegrationTest {
            id: format!("test_{}_unauthorized", self.sanitize_name(&endpoint.path)),
            name: format!("test_{}_unauthorized", self.sanitize_name(&endpoint.path)),
            description: format!("Test {} {} without auth returns 401", format!("{:?}", endpoint.method), endpoint.path),
            endpoints: vec![endpoint.path.clone()],
            steps: vec![
                TestStep {
                    step_number: 1,
                    action: TestAction::Request,
                    request: Some(HttpRequest {
                        method: endpoint.method,
                        url: endpoint.path.clone(),
                        headers: HashMap::new(),
                        body: None,
                    }),
                    response_check: Some(ResponseCheck {
                        status_code: 401,
                        body_contains: vec!["unauthorized".to_string(), "401".to_string()],
                        json_path_checks: Vec::new(),
                    }),
                    setup: None,
                    teardown: None,
                },
            ],
            fixtures: vec![],
            language: self.config.language,
        })
    }

    /// Generate error case tests.
    fn generate_error_tests(&self, endpoint: &ApiEndpoint) -> Vec<IntegrationTest> {
        let mut tests = Vec::new();

        // Missing required params test
        for param in endpoint.path_params.iter().filter(|p| p.required) {
            let test = IntegrationTest {
                id: format!("test_{}_missing_{}", self.sanitize_name(&endpoint.path), param.name),
                name: format!("test_{}_missing_required_param_{}", self.sanitize_name(&endpoint.path), param.name),
                description: format!("Test {} {} with missing required param {}", format!("{:?}", endpoint.method), endpoint.path, param.name),
                endpoints: vec![endpoint.path.clone()],
                steps: vec![
                    TestStep {
                        step_number: 1,
                        action: TestAction::Request,
                        request: Some(HttpRequest {
                            method: endpoint.method,
                            url: endpoint.path.clone(),
                            headers: HashMap::new(),
                            body: None,
                        }),
                        response_check: Some(ResponseCheck {
                            status_code: 400,
                            body_contains: vec![param.name.clone()],
                            json_path_checks: Vec::new(),
                        }),
                        setup: None,
                        teardown: None,
                    },
                ],
                fixtures: vec![],
                language: self.config.language,
            };
            tests.push(test);
        }

        tests
    }

    /// Generate request code.
    fn generate_request(&self, endpoint: &ApiEndpoint) -> HttpRequest {
        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        if endpoint.auth_required {
            headers.insert("Authorization".to_string(), "Bearer token".to_string());
        }

        HttpRequest {
            method: endpoint.method,
            url: endpoint.path.clone(),
            headers,
            body: endpoint.request_body.as_ref().map(|b| self.generate_sample_body(b)),
        }
    }

    /// Generate sample request body.
    fn generate_sample_body(&self, body_type: &str) -> String {
        if body_type.contains("User") || body_type.contains("user") {
            r#"{"name": "test_user", "email": "test@example.com"}"#.to_string()
        } else if body_type.contains("Product") || body_type.contains("product") {
            r#"{"name": "Test Product", "price": 99.99}"#.to_string()
        } else if body_type.contains("Order") || body_type.contains("order") {
            r#"{"user_id": 1, "items": [{"product_id": 1, "quantity": 2}]}"#.to_string()
        } else {
            r#"{"data": "sample"}"#.to_string()
        }
    }

    /// Generate setup code.
    fn generate_setup_code(&self, endpoint: &ApiEndpoint) -> String {
        match self.config.framework {
            IntegrationFramework::RustActix | IntegrationFramework::RustAxum => {
                format!("// Setup test database for {}\nlet app = test_app().await;", endpoint.path)
            }
            IntegrationFramework::PythonFlask | IntegrationFramework::PythonFastApi => {
                format!("# Setup test client for {}\nclient = TestClient(app)", endpoint.path)
            }
            IntegrationFramework::NodeExpress => {
                format!("// Setup supertest agent for {}\nconst agent = request.agent(app);", endpoint.path)
            }
            IntegrationFramework::GoGin => {
                format!("// Setup gin test router for {}\nrouter := SetupTestRouter()", endpoint.path)
            }
        }
    }

    /// Generate test fixtures.
    fn generate_fixtures(&self, endpoint: &ApiEndpoint) -> Vec<Fixture> {
        let mut fixtures = Vec::new();

        if endpoint.auth_required {
            fixtures.push(Fixture {
                name: "auth_token".to_string(),
                fixture_type: FixtureType::Mock,
                data: serde_json::json!({
                    "token": "test_token_123",
                    "expires_at": "2025-12-31T23:59:59Z"
                }),
            });
        }

        if endpoint.request_body.is_some() {
            fixtures.push(Fixture {
                name: "test_data".to_string(),
                fixture_type: FixtureType::Mock,
                data: serde_json::json!({
                    "id": 1,
                    "name": "Test Entity",
                    "created_at": "2025-01-01T00:00:00Z"
                }),
            });
        }

        fixtures
    }

    /// Sanitize name for use in identifiers.
    fn sanitize_name(&self, path: &str) -> String {
        path.replace(['/', '-'], "_")
            .replace(":", "")
            .trim_matches('_')
            .to_string()
    }

    /// Render test to code string.
    pub fn render(&self, test: &IntegrationTest) -> String {
        match self.config.language {
            TestLanguage::Rust => self.render_rust(test),
            TestLanguage::Python => self.render_python(test),
            TestLanguage::JavaScript | TestLanguage::TypeScript => self.render_js(test),
            TestLanguage::Go => self.render_go(test),
        }
    }

    fn render_rust(&self, test: &IntegrationTest) -> String {
        let mut code = String::new();
        code.push_str("#[tokio::test]\n");
        code.push_str(&format!("async fn {}() {{\n", test.name));
        code.push_str("    // Setup\n");

        for fixture in &test.fixtures {
            code.push_str(&format!("    let {} = {:#};\n", fixture.name, fixture.data));
        }

        code.push_str("\n    // Test steps\n");
        for step in &test.steps {
            match step.action {
                TestAction::Request => {
                    if let Some(req) = &step.request {
                        code.push_str(&format!("    let response = client.{}(", format!("{:?}", req.method).to_lowercase()));
                        code.push_str(&format!("\"{}\")", req.url));

                        if !req.headers.is_empty() {
                            code.push_str("\n        .header(\"Authorization\", \"Bearer token\")");
                        }

                        if let Some(body) = &req.body {
                            code.push_str(&format!("\n        .json(&{})", body));
                        }

                        code.push_str("\n        .await\n");
                        code.push_str("        .expect(\"request failed\");\n");
                    }

                    if let Some(check) = &step.response_check {
                        code.push_str(&format!("    assert_eq!(response.status(), StatusCode::{})\n", check.status_code));
                    }
                }
                TestAction::Assert => {
                    if let Some(check) = &step.response_check {
                        for item in &check.body_contains {
                            code.push_str(&format!("    assert!(response.body().contains(\"{}\"));\n", item));
                        }
                    }
                }
                _ => {}
            }
        }

        code.push_str("}\n");
        code
    }

    fn render_python(&self, test: &IntegrationTest) -> String {
        let mut code = String::new();
        code.push_str(&format!("def test_{}():\n", test.name.replace('-', "_")));
        code.push_str("    # Setup\n");

        for fixture in &test.fixtures {
            code.push_str(&format!("    {} = {}\n", fixture.name, fixture.data));
        }

        code.push_str("\n    # Test steps\n");
        for step in &test.steps {
            if let TestAction::Request = step.action {
                if let Some(req) = &step.request {
                    let method = format!("{:?}", req.method).to_string().to_lowercase();
                    code.push_str(&format!("    response = client.{}(\"{}\")\n", method, req.url));

                    if let Some(check) = &step.response_check {
                        code.push_str(&format!("    assert response.status_code == {}\n", check.status_code));
                    }
                }
            }
        }

        code
    }

    fn render_js(&self, test: &IntegrationTest) -> String {
        let mut code = String::new();
        code.push_str(&format!("describe('{}', () => {{\n", test.name));
        code.push_str("  it('should work', async () => {\n");

        for step in &test.steps {
            if let TestAction::Request = step.action {
                if let Some(req) = &step.request {
                    let method = format!("{:?}", req.method).to_string().to_lowercase();
                    code.push_str(&format!("    const res = await agent.{}(\"{}\");\n", method, req.url));

                    if let Some(check) = &step.response_check {
                        code.push_str(&format!("    expect(res.status).toBe({});\n", check.status_code));
                    }
                }
            }
        }

        code.push_str("  });\n});\n");
        code
    }

    fn render_go(&self, test: &IntegrationTest) -> String {
        let mut code = String::new();
        code.push_str(&format!("func Test{}(t *testing.T) {{\n", test.name.replace('-', "")));
        code.push_str("    // Setup\n");
        code.push_str("    router := SetupTestRouter()\n\n");
        code.push_str("    // Test\n");

        for step in &test.steps {
            if let TestAction::Request = step.action {
                if let Some(req) = &step.request {
                    let method = format!("{:?}", req.method).to_string().to_lowercase();
                    code.push_str("    w := httptest.NewRecorder()\n");
                    code.push_str(&format!("    req, _ := http.NewRequest(\"{}\", \"{}\", nil)\n", method.to_uppercase(), req.url));
                    code.push_str("    router.ServeHTTP(w, req)\n");

                    if let Some(check) = &step.response_check {
                        code.push_str(&format!("    assert.Equal(t, {}, w.Code)\n", check.status_code));
                    }
                }
            }
        }

        code.push_str("}\n");
        code
    }
}

/// API endpoint parser from source code.
pub struct ApiEndpointParser;

impl ApiEndpointParser {
    /// Parse API endpoints from Rust source code.
    pub fn parse_rust(source: &str) -> Vec<ApiEndpoint> {
        let mut endpoints = Vec::new();

        // Simple regex-like parsing for route macros
        for line in source.lines() {
            let trimmed = line.trim();

            // Detect route macros like #[get("/path")], #[post("/path")]
            if trimmed.starts_with("#[get(") || trimmed.starts_with("#[post(") ||
               trimmed.starts_with("#[put(") || trimmed.starts_with("#[delete(") ||
               trimmed.starts_with("#[patch(") {
                let method = if trimmed.starts_with("#[get(") {
                    HttpMethod::GET
                } else if trimmed.starts_with("#[post(") {
                    HttpMethod::POST
                } else if trimmed.starts_with("#[put(") {
                    HttpMethod::PUT
                } else if trimmed.starts_with("#[delete(") {
                    HttpMethod::DELETE
                } else if trimmed.starts_with("#[patch(") {
                    HttpMethod::PATCH
                } else {
                    continue;
                };

                // Extract path
                if let Some(path) = trimmed.split('(').nth(1).and_then(|s| s.split(')').next()) {
                    endpoints.push(ApiEndpoint {
                        method,
                        path: path.to_string(),
                        path_params: Vec::new(),
                        query_params: Vec::new(),
                        request_body: None,
                        response_body: None,
                        status_codes: vec![200],
                        auth_required: false,
                    });
                }
            }
        }

        endpoints
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rust_endpoints() {
        let source = r#"
#[get("/users")]
async fn get_users() {}

#[post("/users")]
async fn create_user() {}

#[get("/users/{id}")]
async fn get_user() {}
"#;

        let endpoints = ApiEndpointParser::parse_rust(source);
        assert_eq!(endpoints.len(), 3);
        assert_eq!(endpoints[0].method, HttpMethod::GET);
        assert_eq!(endpoints[0].path, "/users");
    }

    #[test]
    fn test_generate_integration_test() {
        let endpoint = ApiEndpoint {
            method: HttpMethod::GET,
            path: "/users".to_string(),
            path_params: Vec::new(),
            query_params: Vec::new(),
            request_body: None,
            response_body: Some("User".to_string()),
            status_codes: vec![200],
            auth_required: true,
        };

        let generator = IntegrationTestGenerator::new(IntegrationTestConfig::default());
        let tests = generator.generate(&[endpoint]);

        assert!(!tests.is_empty());
    }
}
