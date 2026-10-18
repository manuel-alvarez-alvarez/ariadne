//! Dashboard repository.
use crate::defaults::BUILTIN_DASHBOARDS;
use crate::query::Filtered;
use crate::{Change, Dashboard, Result, Store, StoreError, now};
#[derive(Debug, Clone)]
pub struct NewDashboard {
    pub name: String,
    pub document: String,
}
impl Store {
    pub(crate) async fn seed_builtin_dashboards(&self) -> Result<()> {
        let mut tx = self.w().begin().await?;
        let ts = now();
        for d in &BUILTIN_DASHBOARDS {
            sqlx::query("INSERT INTO dashboards (name,document,builtin,created_at,updated_at) VALUES (?,NULL,1,?,?) ON CONFLICT(name) DO UPDATE SET builtin=1,updated_at=excluded.updated_at WHERE dashboards.builtin=0").bind(d.name).bind(&ts).bind(&ts).execute(&mut *tx).await?;
        }
        let shipped = vec!["?"; BUILTIN_DASHBOARDS.len()].join(", ");
        let mut adopt = sqlx::query(sqlx::AssertSqlSafe(format!("UPDATE dashboards SET builtin=0,updated_at=? WHERE builtin=1 AND document IS NOT NULL AND name NOT IN ({shipped})"))).bind(&ts);
        let mut drop = sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM dashboards WHERE builtin=1 AND document IS NULL AND name NOT IN ({shipped})"
        )));
        for d in &BUILTIN_DASHBOARDS {
            adopt = adopt.bind(d.name);
            drop = drop.bind(d.name);
        }
        adopt.execute(&mut *tx).await?;
        drop.execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn create_dashboard(&self, n: NewDashboard) -> Result<Dashboard> {
        validate(&n.name, &n.document)?;
        let ts = now();
        sqlx::query("INSERT INTO dashboards (name,document,builtin,created_at,updated_at) VALUES (?,?,0,?,?)").bind(&n.name).bind(&n.document).bind(&ts).bind(&ts).execute(self.w()).await.map_err(|e|taken(e,&n.name))?;
        let d = self.get_dashboard(&n.name).await?;
        self.publish(Change::DashboardCreated(d.clone()));
        Ok(d)
    }
    pub async fn get_dashboard(&self, name: &str) -> Result<Dashboard> {
        self.fetch_by("dashboard", "dashboards", "name", name).await
    }
    pub async fn list_dashboards(&self) -> Result<Vec<Dashboard>> {
        Filtered::new("dashboards")
            .fetch(self, " ORDER BY name", &[])
            .await
    }
    pub async fn set_dashboard_document(&self, name: &str, document: &str) -> Result<Dashboard> {
        match self.get_dashboard(name).await {
            Err(StoreError::NotFound { .. }) => {
                return self
                    .create_dashboard(NewDashboard {
                        name: name.into(),
                        document: document.into(),
                    })
                    .await;
            }
            Err(e) => return Err(e),
            Ok(_) => {}
        }
        validate(name, document)?;
        sqlx::query("UPDATE dashboards SET document=?,updated_at=? WHERE name=?")
            .bind(document)
            .bind(now())
            .bind(name)
            .execute(self.w())
            .await?;
        let d = self.get_dashboard(name).await?;
        self.publish(Change::DashboardUpdated(d.clone()));
        Ok(d)
    }
    pub async fn reset_dashboard(&self, name: &str) -> Result<Dashboard> {
        let d = self.get_dashboard(name).await?;
        if !d.is_builtin() {
            return Err(StoreError::Conflict(format!(
                "dashboard {name} is not one Ariadne ships"
            )));
        }
        sqlx::query("UPDATE dashboards SET document=NULL,updated_at=? WHERE name=?")
            .bind(now())
            .bind(name)
            .execute(self.w())
            .await?;
        let d = self.get_dashboard(name).await?;
        self.publish(Change::DashboardUpdated(d.clone()));
        Ok(d)
    }
    pub async fn delete_dashboard(&self, name: &str) -> Result<()> {
        let d = self.get_dashboard(name).await?;
        if d.is_builtin() {
            return Err(StoreError::DashboardBuiltin(name.into()));
        }
        sqlx::query("DELETE FROM dashboards WHERE name=?")
            .bind(name)
            .execute(self.w())
            .await?;
        self.publish(Change::DashboardDeleted(d));
        Ok(())
    }
}
fn validate(name: &str, document: &str) -> Result<()> {
    let v: serde_json::Value = serde_json::from_str(document)
        .map_err(|e| StoreError::Invalid(format!("document is not JSON: {e}")))?;
    if v.get("kind").and_then(serde_json::Value::as_str) != Some("Dashboard") {
        return Err(StoreError::Invalid(
            "document.kind must be Dashboard".into(),
        ));
    }
    if v.pointer("/metadata/name")
        .and_then(serde_json::Value::as_str)
        != Some(name)
    {
        return Err(StoreError::Invalid(format!(
            "document.metadata.name must be {name}"
        )));
    }
    if !v.get("spec").is_some_and(serde_json::Value::is_object) {
        return Err(StoreError::Invalid(
            "document.spec must be an object".into(),
        ));
    }
    Ok(())
}
fn taken(e: sqlx::Error, name: &str) -> StoreError {
    match e {
        sqlx::Error::Database(ref d) if d.is_unique_violation() => {
            StoreError::Conflict(format!("dashboard name already exists: {name}"))
        }
        o => StoreError::Db(o),
    }
}
