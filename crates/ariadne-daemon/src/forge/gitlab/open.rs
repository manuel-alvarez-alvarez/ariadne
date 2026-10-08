//! Opening a merge request, through `glab mr create`.

use super::Gitlab;
use super::details::repo_url;

impl Gitlab {
    /// `glab mr create -R https://<host>/<group>/<name> --source-branch
    /// <head> --target-branch <base> --title <title> --description <body>
    /// [--draft]`: the URL `glab` answers with. `repo` is `host/group/name`,
    /// and `-R` takes the project's URL, which names its host.
    pub async fn open(
        &self,
        repo: &str,
        head: &str,
        base: &str,
        title: &str,
        body: &str,
        draft: bool,
    ) -> Result<String, String> {
        let project = repo_url(repo)?;
        let mut args = vec![
            "mr",
            "create",
            "-R",
            &project,
            "--source-branch",
            head,
            "--target-branch",
            base,
            "--title",
            title,
            "--description",
            body,
        ];
        if draft {
            args.push("--draft");
        }
        self.cli.answer(&args).await
    }
}
