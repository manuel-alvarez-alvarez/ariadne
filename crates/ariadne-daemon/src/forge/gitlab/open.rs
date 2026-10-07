//! Opening a merge request, through `glab mr create`.

use super::Gitlab;

impl Gitlab {
    /// `glab mr create -R <owner>/<name> --source-branch <head>
    /// --target-branch <base> --title <title> --description <body>
    /// [--draft]`: the URL `glab` answers with.
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
            "mr",
            "create",
            "-R",
            repo,
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
