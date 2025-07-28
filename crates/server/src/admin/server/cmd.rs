use std::{fmt::Write, path::PathBuf, sync::Arc};

use crate::{
    AppError, AppResult, info,
    utils::{stream::IterStream, time},
    warn,
};
use futures_util::TryStreamExt;

use crate::macros::admin_command;

pub(super) async fn uptime(ctx: &Context<'_>) -> AppResult<()> {
    let elapsed = self.services.server.started.elapsed().expect("standard duration");

    let result = time::pretty(elapsed);
    ctx.write_str(&format!("{result}.")).await
}

pub(super) async fn show_config(ctx: &Context<'_>) -> AppResult<()> {
    ctx.write_str(&format!("{}", *self.services.server.config)).await
}

pub(super) async fn reload_config(ctx: &Context<'_>, path: Option<PathBuf>) -> AppResult<()> {
    let path = path.as_deref().into_iter();
    self.services.config.reload(path)?;

    ctx.write_str("Successfully reconfigured.").await
}

pub(super) async fn list_features(ctx: &Context<'_>, available: bool, enabled: bool, comma: bool) -> AppResult<()> {
    let delim = if comma { "," } else { " " };
    if enabled && !available {
        let features = info::rustc::features().join(delim);
        let out = format!("`\n{features}\n`");
        return ctx.write_str(&out).await;
    }

    if available && !enabled {
        let features = info::cargo::features().join(delim);
        let out = format!("`\n{features}\n`");
        return ctx.write_str(&out).await;
    }

    let mut features = String::new();
    let enabled = info::rustc::features();
    let available = info::cargo::features();
    for feature in available {
        let active = enabled.contains(&feature.as_str());
        let emoji = if active { "✅" } else { "❌" };
        let remark = if active { "[enabled]" } else { "" };
        writeln!(features, "{emoji} {feature} {remark}")?;
    }

    ctx.write_str(&features).await
}

// pub(super) async fn clear_caches(ctx: &Context<'_>) -> AppResult<()> {
// 	clear_cache(ctx).await;

// 	ctx.write_str("Done.").await
// }

pub(super) async fn list_backups(ctx: &Context<'_>) -> AppResult<()> {
    self.services
        .db
        .db
        .backup_list()?
        .try_stream()
        .try_for_each(|result| write!(self, "{result}"))
        .await
}

pub(super) async fn backup_database(ctx: &Context<'_>) -> AppResult<()> {
    let db = Arc::clone(&self.services.db);
    let result = self
        .services
        .server
        .runtime()
        .spawn_blocking(move || match db.db.backup() {
            Ok(()) => "Done".to_owned(),
            Err(e) => format!("Failed: {e}"),
        })
        .await?;

    let count = self.services.db.db.backup_count()?;
    ctx.write_str(&format!("{result}. Currently have {count} backups."))
        .await
}

pub(super) async fn admin_notice(ctx: &Context<'_>, message: Vec<String>) -> AppResult<()> {
    let message = message.join(" ");
    self.services.admin.send_text(&message).await;

    ctx.write_str("Notice was sent to #admins").await
}

pub(super) async fn reload_mods(ctx: &Context<'_>) -> AppResult<()> {
    self.services.server.reload()?;

    ctx.write_str("Reloading server...").await
}

#[cfg(unix)]
pub(super) async fn restart(ctx: &Context<'_>, force: bool) -> AppResult<()> {
    use crate::utils::sys::current_exe_deleted;

    if !force && current_exe_deleted() {
        return Err(AppError::public(
            "The server cannot be restarted because the executable changed. If this is expected \
			 use --force to override.",
        ));
    }

    self.services.server.restart()?;

    ctx.write_str("Restarting server...").await
}

pub(super) async fn shutdown(ctx: &Context<'_>) -> AppResult<()> {
    warn!("shutdown command");
    self.services.server.shutdown()?;

    ctx.write_str("Shutting down server...").await
}
