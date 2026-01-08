use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ProgramsWithCondition::Table)
                    .if_not_exists()
                    .col(pk_uuid(ProgramsWithCondition::Id))
                    .col(string(ProgramsWithCondition::Name))
                    .col(text(ProgramsWithCondition::GithubUrl))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx-programs_cond_github_url")
                    .unique()
                    .table(ProgramsWithCondition::Table)
                    .col(ProgramsWithCondition::GithubUrl)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, _: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
#[derive(DeriveIden)]
enum ProgramsWithCondition {
    Table,
    Id,
    Name,
    GithubUrl,
}
