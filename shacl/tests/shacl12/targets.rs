#[cfg(test)]
mod tests {
    use crate::common::TestSuiteError;
    use crate::test;
    use shacl::validator::ShaclValidationMode;

    const PATH: &str = "tests/shacl12/targets/";

    #[test]
    fn target_where_001() -> Result<(), TestSuiteError> {
        let path = format!("{}/{}.ttl", PATH, "targetWhere-001");
        test(path, ShaclValidationMode::Native)
    }
}
