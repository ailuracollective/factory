pub use sea_orm_migration::prelude::*;

mod m20240101_000001_initial;
mod m20240101_000002_policies;
mod m20240101_000003_runner_tokens;
mod m20240101_000004_runner_callback;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20240101_000001_initial::Migration),
            Box::new(m20240101_000002_policies::Migration),
            Box::new(m20240101_000003_runner_tokens::Migration),
            Box::new(m20240101_000004_runner_callback::Migration),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_ordered_and_unique() {
        let migrations = Migrator::migrations();
        let names: Vec<_> = migrations.iter().map(|m| m.name().to_string()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "migrations must be in chronological order");
        let unique: std::collections::BTreeSet<_> = names.iter().collect();
        assert_eq!(unique.len(), names.len(), "migration names must be unique");
    }
}
