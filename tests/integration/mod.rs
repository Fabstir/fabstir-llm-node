// tests/integration/mod.rs
// Integration test modules

#[cfg(test)]
pub mod mock {
    pub mod test_cache_flow;
    pub mod test_e2e_workflow;
}

#[cfg(test)]
pub mod test_e2e_encryption;

#[cfg(test)]
pub mod test_host_management;
