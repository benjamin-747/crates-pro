use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use crate::{git, utils, BoxError};
use database::storage::Context;
use futures::TryStreamExt;
use sea_orm::{ActiveValue::Set, IntoActiveModel};

pub async fn recently_update(context: Context) -> Result<(), BoxError> {
    let stg = context.github_handler_stg();
    let program_stream = stg.query_non_recently_programs_stream().await.unwrap();
    let counter = Arc::new(AtomicUsize::new(0));

    program_stream
        .try_for_each_concurrent(16, |model| {
            let context = context.clone();
            let base_dir = context.base_dir.clone();
            let program = model.clone();
            let stg = stg.clone();
            let counter = counter.clone();

            async move {
                if let Some((owner, repo)) = utils::parse_to_owner_and_repo(&program.github_url) {
                    let nested_path = utils::repo_dir(base_dir, &owner, &repo);
                    if nested_path.exists() {
                        let res = git::check_commit_last_three_month(&nested_path).await;
                        let mut a_model = model.into_active_model();
                        a_model.recently_update = Set(Some(res));
                        stg.update_program(a_model).await?;
                    } else {
                        tracing::error!("repo not cloned, skip {:?}", nested_path);
                    }
                }
                let count = counter.fetch_add(1, Ordering::SeqCst) + 1;
                if count % 1000 == 0 {
                    tracing::info!("已经处理了 {} 个 program", count);
                }
                Ok(())
            }
        })
        .await?;
    Ok(())
}
