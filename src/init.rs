//! `oaudit init` — scaffold .oaudit/ in the target directory.

use anyhow::Result;
use std::path::PathBuf;

pub async fn scaffold(_root: PathBuf) -> Result<()> {
    anyhow::bail!("init::scaffold not yet implemented")
}
