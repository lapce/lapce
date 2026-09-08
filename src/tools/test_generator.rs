//! Unit Test Generator - Generates test cases from function signatures.
//!
//! This module provides:
//! - Automatic test case generation from function signatures
//! - Boundary value analysis
//! - Edge case detection
//! - Multiple testing framework support (Rust, Python, JavaScript, etc.)

use std::collections::HashMap;

/// A generated test case.
#[derive(Debug, Clone)]
pub struct TestCase {
    pub id: String,
    pub name: String,
    pub code: String,
    pub inputs: Vec<TestInput>,
    pub expected: Option<String>,
    pub category: TestCategory,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestCategory {
    HappyPath,
    Boundary,
    ErrorCase,
    NullCase,
    EmptyCase,
    StressCase,
}

#[derive(Debug, Clone)]
pub struct TestInput {
    pub name: String,
    pub param_type: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct FunctionSignature {
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<String>,
    pub is_async: bool,
    pub may_panic: bool,
}

#[derive(Debug, Clone)]
pub struct Parameter {
    pub name: String,
    pub param_type: String,
    pub is_optional: bool,
    pub default_value: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TestGenConfig {
    pub language: TestLanguage,
    pub framework: TestFramework,
    pub include_doctests: bool,
    pub include_property_tests: bool,
    pub boundary_cases: usize,
    pub random_cases: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TestLanguage {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Go,
    Java,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestFramework {
    RustTest,
    Pytest,
    Jest,
    GoTest,
    JUnit,
}

impl Default for TestGenConfig {
    fn default() -> Self {
        Self {
            language: TestLanguage::Rust,
            framework: TestFramework::RustTest,
            include_doctests: true,
            include_property_tests: false,
            boundary_cases: 3,
            random_cases: 2,
        }
    }
}

pub struct TestGenerator {
    config: TestGenConfig,
}

impl TestGenerator {
    pub fn new(config: TestGenConfig) -> Self {
        Self { config }
    }

    pub fn generate_tests(&self, sig: &FunctionSignature) -> Vec<TestCase> {
        let mut cases = Vec::new();
        cases.extend(self.generate_happy_path_tests(sig));
        cases.extend(self.generate_boundary_tests(sig));
        cases.extend(self.generate_null_tests(sig));
        cases.extend(self.generate_empty_tests(sig));
        cases.extend(self.generate_error_tests(sig));
        cases
    }

    fn generate_happy_path_tests(&self, sig: &FunctionSignature) -> Vec<TestCase> {
        let inputs: Vec<TestInput> = sig.parameters.iter().map(|p| TestInput {
            name: p.name.clone(),
            param_type: p.param_type.clone(),
            value: self.generate_normal_value(p),
        }).collect();

        vec![TestCase {
            id: format!("{}_happy_1", sig.name),
            name: format!("test_{}_happy_path", sig.name),
            code: self.render_test(sig, &inputs, &sig.return_type.clone(), TestCategory::HappyPath),
            inputs,
            expected: sig.return_type.as_ref().map(|_| "expected_value".to_string()),
            category: TestCategory::HappyPath,
            description: "Basic happy path test".to_string(),
        }]
    }

    fn generate_boundary_tests(&self, sig: &FunctionSignature) -> Vec<TestCase> {
        let mut cases = Vec::new();

        for (i, param) in sig.parameters.iter().enumerate() {
            let boundaries = self.get_boundary_values(param);

            for (j, boundary) in boundaries.iter().enumerate().take(self.config.boundary_cases) {
                let inputs: Vec<TestInput> = sig.parameters.iter().map(|p| {
                    if p.name == param.name {
                        TestInput { name: p.name.clone(), param_type: p.param_type.clone(), value: boundary.clone() }
                    } else {
                        TestInput { name: p.name.clone(), param_type: p.param_type.clone(), value: self.generate_normal_value(p) }
                    }
                }).collect();

                cases.push(TestCase {
                    id: format!("{}_boundary_{}_{}", sig.name, i, j),
                    name: format!("test_{}_boundary_{}_{}", sig.name, i, j),
                    code: self.render_test(sig, &inputs, &sig.return_type.clone(), TestCategory::Boundary),
                    inputs,
                    expected: sig.return_type.as_ref().map(|_| "expected".to_string()),
                    category: TestCategory::Boundary,
                    description: format!("Boundary test for {}", param.name),
                });
            }
        }
        cases
    }

    fn generate_null_tests(&self, sig: &FunctionSignature) -> Vec<TestCase> {
        let mut cases = Vec::new();

        for (i, param) in sig.parameters.iter().enumerate() {
            if !param.is_optional {
                let inputs: Vec<TestInput> = sig.parameters.iter().map(|p| {
                    if p.name == param.name {
                        TestInput { name: p.name.clone(), param_type: p.param_type.clone(), value: self.get_null_value(&p.param_type) }
                    } else {
                        TestInput { name: p.name.clone(), param_type: p.param_type.clone(), value: self.generate_normal_value(p) }
                    }
                }).collect();

                cases.push(TestCase {
                    id: format!("{}_null_{}", sig.name, i),
                    name: format!("test_{}_handles_null_{}", sig.name, i),
                    code: self.render_test(sig, &inputs, &None, TestCategory::NullCase),
                    inputs,
                    expected: None,
                    category: TestCategory::NullCase,
                    description: format!("Test null handling for {}", param.name),
                });
            }
        }
        cases
    }

    fn generate_empty_tests(&self, sig: &FunctionSignature) -> Vec<TestCase> {
        let mut cases = Vec::new();

        for (i, param) in sig.parameters.iter().enumerate() {
            if self.is_collection_type(&param.param_type) {
                let inputs: Vec<TestInput> = sig.parameters.iter().map(|p| {
                    TestInput {
                        name: p.name.clone(),
                        param_type: p.param_type.clone(),
                        value: if p.name == param.name { self.get_empty_value(&param.param_type) } else { self.generate_normal_value(p) },
                    }
                }).collect();

                cases.push(TestCase {
                    id: format!("{}_empty_{}", sig.name, i),
                    name: format!("test_{}_handles_empty_{}", sig.name, i),
                    code: self.render_test(sig, &inputs, &sig.return_type.clone(), TestCategory::EmptyCase),
                    inputs,
                    expected: sig.return_type.as_ref().map(|_| "expected".to_string()),
                    category: TestCategory::EmptyCase,
                    description: format!("Test empty input for {}", param.name),
                });
            }
        }
        cases
    }

    fn generate_error_tests(&self, sig: &FunctionSignature) -> Vec<TestCase> {
        let mut cases = Vec::new();

        if sig.may_panic {
            let inputs: Vec<TestInput> = sig.parameters.iter().map(|p| TestInput {
                name: p.name.clone(),
                param_type: p.param_type.clone(),
                value: self.get_invalid_value(p),
            }).collect();

            cases.push(TestCase {
                id: format!("{}_panic", sig.name),
                name: format!("test_{}_panics_on_invalid_input", sig.name),
                code: self.render_test(sig, &inputs, &None, TestCategory::ErrorCase),
                inputs,
                expected: None,
                category: TestCategory::ErrorCase,
                description: "Test panic on invalid input".to_string(),
            });
        }
        cases
    }

    fn generate_normal_value(&self, param: &Parameter) -> String {
        if let Some(ref default) = param.default_value {
            return default.clone();
        }

        match param.param_type.as_str() {
            "i32" | "i64" | "isize" => "42".to_string(),
            "u32" | "u64" | "usize" => "10".to_string(),
            "f32" | "f64" => "3.14".to_string(),
            "bool" => "true".to_string(),
            "String" | "&str" => "\"hello\"".to_string(),
            _ => "Default::default()".to_string(),
        }
    }

    fn get_boundary_values(&self, param: &Parameter) -> Vec<String> {
        match param.param_type.as_str() {
            "i32" | "i64" | "isize" => vec!["0".to_string(), "-1".to_string(), "i32::MAX".to_string()],
            "u32" | "u64" | "usize" => vec!["0".to_string(), "1".to_string(), "u32::MAX".to_string()],
            "f32" | "f64" => vec!["0.0".to_string(), "f64::MAX".to_string(), "f64::INFINITY".to_string()],
            _ => vec![],
        }
    }

    fn get_null_value(&self, t: &str) -> String {
        match t {
            "String" => "String::new()".to_string(),
            "&str" => "\"\"".to_string(),
            "Option<T>" => "None".to_string(),
            _ => "None".to_string(),
        }
    }

    fn get_empty_value(&self, t: &str) -> String {
        if t.starts_with("Vec<") { "vec![]".to_string() }
        else if t.starts_with("HashMap<") { "HashMap::new()".to_string() }
        else if t.starts_with("String") { "String::new()".to_string() }
        else { "vec![]".to_string() }
    }

    fn get_invalid_value(&self, param: &Parameter) -> String {
        match param.param_type.as_str() {
            "i32" | "i64" => "-999999".to_string(),
            "f32" | "f64" => "f64::NAN".to_string(),
            "String" | "&str" => "\"invalid!@#$%\"".to_string(),
            _ => "panic!".to_string(),
        }
    }

    fn is_collection_type(&self, t: &str) -> bool {
        t.starts_with("Vec<") || t.starts_with("HashMap<") || t.starts_with("String") || t == "&str"
    }

    fn render_test(&self, sig: &FunctionSignature, inputs: &[TestInput], expected: &Option<String>, category: TestCategory) -> String {
        match self.config.language {
            TestLanguage::Rust => self.render_rust_test(sig, inputs, expected, category),
            TestLanguage::Python => self.render_python_test(sig, inputs, expected, category),
            TestLanguage::JavaScript | TestLanguage::TypeScript => self.render_js_test(sig, inputs, expected, category),
            _ => self.render_rust_test(sig, inputs, expected, category),
        }
    }

    fn render_rust_test(&self, sig: &FunctionSignature, inputs: &[TestInput], expected: &Option<String>, category: TestCategory) -> String {
        let call_args: Vec<String> = inputs.iter().map(|i| i.value.clone()).collect();

        let mut code = format!("#[test]\nfn {}() {{\n    // {:?}\n", sig.name.replace(" ", "_").to_lowercase(), category);

        if !inputs.is_empty() {
            code.push_str("    // Setup\n");
            for inp in inputs {
                code.push_str(&format!("    let {}: {} = {};\n", inp.name, inp.param_type, inp.value));
            }
            code.push('\n');
        }

        code.push_str("    // Act & Assert\n");
        if let Some(exp) = expected {
            code.push_str(&format!("    let result = {}({});\n", sig.name, call_args.join(", ")));
            code.push_str(&format!("    assert_eq!(result, {});\n", exp));
        } else if sig.may_panic {
            code.push_str(&format!("    // {}({}); // Should panic\n", sig.name, call_args.join(", ")));
        } else {
            code.push_str(&format!("    {}({});\n", sig.name, call_args.join(", ")));
        }

        code.push_str("}\n");
        code
    }

    fn render_python_test(&self, sig: &FunctionSignature, inputs: &[TestInput], expected: &Option<String>, category: TestCategory) -> String {
        let call_args: Vec<String> = inputs.iter().map(|i| i.value.replace("\"", "'")).collect();

        let mut code = format!("def test_{}():\n    \"\"\"{:?}\"\"\"\n", sig.name.replace(" ", "_").to_lowercase(), category);

        if !inputs.is_empty() {
            code.push_str("    # Setup\n");
            for inp in inputs {
                code.push_str(&format!("    {} = {}\n", inp.name, inp.value.replace("\"", "'")));
            }
            code.push('\n');
        }

        code.push_str("    # Act & Assert\n");
        if let Some(exp) = expected {
            code.push_str(&format!("    result = {}({})\n", sig.name, call_args.join(", ")));
            code.push_str(&format!("    assert result == {}\n", exp.replace("\"", "'")));
        } else {
            code.push_str(&format!("    {}({})\n", sig.name, call_args.join(", ")));
        }

        code
    }

    fn render_js_test(&self, sig: &FunctionSignature, inputs: &[TestInput], expected: &Option<String>, category: TestCategory) -> String {
        let call_args: Vec<String> = inputs.iter().map(|i| i.value.clone()).collect();

        let mut code = format!("describe('{}', () => {{\n  it('{:?}', () => {{\n", sig.name, category);

        if !inputs.is_empty() {
            code.push_str("    // Setup\n");
            for inp in inputs {
                code.push_str(&format!("    const {} = {};\n", inp.name, inp.value));
            }
            code.push('\n');
        }

        code.push_str("    // Act & Assert\n");
        if let Some(exp) = expected {
            code.push_str(&format!("    const result = {}({});\n", sig.name, call_args.join(", ")));
            code.push_str(&format!("    expect(result).toEqual({});\n", exp));
        } else {
            code.push_str(&format!("    {}({});\n", sig.name, call_args.join(", ")));
        }

        code.push_str("  });\n});\n");
        code
    }
}

pub struct TestGenManager {
    generators: HashMap<TestLanguage, TestGenerator>,
}

impl TestGenManager {
    pub fn new() -> Self {
        Self { generators: HashMap::new() }
    }

    pub fn register(&mut self, language: TestLanguage, generator: TestGenerator) {
        self.generators.insert(language, generator);
    }

    pub fn generate(&self, sig: &FunctionSignature, language: TestLanguage) -> Vec<TestCase> {
        self.generators.get(&language)
            .map(|g| g.generate_tests(sig))
            .unwrap_or_else(|| {
                let default = TestGenerator::new(TestGenConfig { language, ..Default::default() });
                default.generate_tests(sig)
            })
    }
}

impl Default for TestGenManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_rust_tests() {
        let generator = TestGenerator::new(TestGenConfig { language: TestLanguage::Rust, ..Default::default() });

        let sig = FunctionSignature {
            name: "add".to_string(),
            parameters: vec![
                Parameter { name: "a".to_string(), param_type: "i32".to_string(), is_optional: false, default_value: None },
                Parameter { name: "b".to_string(), param_type: "i32".to_string(), is_optional: false, default_value: None },
            ],
            return_type: Some("i32".to_string()),
            is_async: false,
            may_panic: false,
        };

        let cases = generator.generate_tests(&sig);
        assert!(!cases.is_empty());
        assert!(cases.iter().any(|c| c.category == TestCategory::HappyPath));
    }

    #[test]
    fn test_boundary_values() {
        let generator = TestGenerator::new(Default::default());
        let param = Parameter { name: "x".to_string(), param_type: "i32".to_string(), is_optional: false, default_value: None };

        let boundaries = generator.get_boundary_values(&param);
        assert!(boundaries.contains(&"0".to_string()));
        assert!(boundaries.contains(&"i32::MAX".to_string()));
    }
}
