//! Opening a pull request, through `gh pr create`.

use super::Github;

impl Github {
    /// `gh pr create --repo <host>/<owner>/<name> --head <head> --base
    /// <base> --title <title> --body <body> [--draft]`: the URL `gh` answers
    /// with. `repo` is `host/owner/name`, which `gh` takes as it is.
    pub async fn open(
        &self,
        repo: &str,
        head: &str,
        base: &str,
        title: &str,
        body: &str,
        draft: bool,
    ) -> Result<String, String> {
        let mut args = vec![
            "pr", "create", "--repo", repo, "--head", head, "--base", base, "--title", title,
            "--body", body,
        ];
        if draft {
            args.push("--draft");
        }
        self.cli.answer(&args).await
    }
}
