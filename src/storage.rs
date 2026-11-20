use crate::{
    model::{Error, Id, Result},
    project::{ChangeSet, Project, SCHEMA_VERSION},
    reporting::StoredResult,
    scenario::{Scenario, ScenarioCollection},
};
use rusqlite::{Connection, OptionalExtension, params};
use std::{fs, path::Path};

/// Exclusive creation: never silently replaces an existing user file.
pub fn export_new(path: impl AsRef<Path>, content: &str) -> Result<()> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(content.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

pub fn read_project(path: impl AsRef<Path>) -> Result<Project> {
    Project::from_json(&fs::read_to_string(path)?)
}

pub struct Store {
    connection: Connection,
}
impl Store {
    pub fn projects(&self) -> Result<Vec<(Id, String)>> {
        let mut statement = self
            .connection
            .prepare("SELECT id,root_json FROM projects ORDER BY id")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut projects = Vec::new();
        for row in rows {
            let (id, json) = row?;
            let id = id
                .parse()
                .map_err(|_| Error::Invalid("identifiant SQLite invalide".into()))?;
            let network: crate::model::Network = serde_json::from_str(&json)?;
            projects.push((id, network.name));
        }
        Ok(projects)
    }
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let connection = Connection::open(path)?;
        connection.execute_batch(
            "PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY, schema_version INTEGER NOT NULL,
                root_version TEXT NOT NULL, root_json TEXT NOT NULL, head TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS changes (
                project_id TEXT NOT NULL REFERENCES projects(id),
                position INTEGER NOT NULL, id TEXT NOT NULL, json TEXT NOT NULL,
                PRIMARY KEY(project_id, position), UNIQUE(project_id, id));
            CREATE TABLE IF NOT EXISTS scenario_collections (
                project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
                network_version TEXT NOT NULL, json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS study_results (
                project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                id TEXT NOT NULL, network_version TEXT NOT NULL,
                kind TEXT NOT NULL, name TEXT NOT NULL, json TEXT NOT NULL,
                PRIMARY KEY(project_id,id));",
        )?;
        Ok(Self { connection })
    }
    pub fn load(&self, id: Id) -> Result<Project> {
        let transaction = self.connection.unchecked_transaction()?;
        let mut project = transaction
            .query_row(
                "SELECT schema_version,root_version,root_json FROM projects WHERE id=?1",
                [id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, u32>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .map(
                |(schema_version, root_version, root_json)| -> Result<Project> {
                    Ok(Project {
                        schema_version,
                        id,
                        root_version: root_version
                            .parse()
                            .map_err(|_| Error::Invalid("version SQLite invalide".into()))?,
                        root: serde_json::from_str(&root_json)?,
                        changes: vec![],
                    })
                },
            )??;
        let mut statement = transaction
            .prepare("SELECT json FROM changes WHERE project_id=?1 ORDER BY position")?;
        for json in statement.query_map([id.to_string()], |r| r.get::<_, String>(0))? {
            project.changes.push(serde_json::from_str(&json?)?);
        }
        project = project.migrate()?;
        let head: String = transaction.query_row(
            "SELECT head FROM projects WHERE id=?1",
            [id.to_string()],
            |r| r.get(0),
        )?;
        if head != project.head().to_string() {
            return Err(Error::Invalid("historique SQLite incohérent".into()));
        }
        drop(statement);
        transaction.commit()?;
        Ok(project)
    }
    /// Only appends to identical history; divergent and stale writers are rejected.
    pub fn save(&mut self, project: &Project) -> Result<()> {
        project.network()?;
        let transaction = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        save_project_transaction(&transaction, project)?;
        transaction.commit()?;
        Ok(())
    }

    /// Atomically saves the network history and its version-bound scenario collection.
    pub fn save_workspace(&mut self, project: &Project, scenarios: &[Scenario]) -> Result<()> {
        project.network()?;
        let collection = ScenarioCollection::new(project, scenarios.to_vec())?;
        let json = collection.to_json(project)?;
        let transaction = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        save_project_transaction(&transaction, project)?;
        save_scenarios_transaction(&transaction, project, &json)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn save_scenarios(&mut self, project: &Project, scenarios: &[Scenario]) -> Result<()> {
        let collection = ScenarioCollection::new(project, scenarios.to_vec())?;
        let json = collection.to_json(project)?;
        let transaction = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        save_scenarios_transaction(&transaction, project, &json)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn load_scenarios(&self, project: &Project) -> Result<Vec<Scenario>> {
        let row: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT network_version,json FROM scenario_collections WHERE project_id=?1",
                [project.id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((network_version, json)) = row else {
            return Ok(vec![]);
        };
        if network_version != project.head().to_string() {
            return Err(Error::Conflict(
                "la collection SQLite appartient à une autre version du réseau".into(),
            ));
        }
        Ok(ScenarioCollection::from_json(project, &json)?.scenarios)
    }

    /// Persist an immutable result envelope. Re-saving identical content is idempotent.
    pub fn save_result(&mut self, project: &Project, result: &StoredResult) -> Result<bool> {
        project.network()?;
        result.validate_for(project)?;
        let json = serde_json::to_string(result)?;
        let transaction = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let existing: Option<String> = transaction
            .query_row(
                "SELECT json FROM study_results WHERE project_id=?1 AND id=?2",
                [project.id.to_string(), result.id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            let same = existing == json;
            transaction.commit()?;
            return if same {
                Ok(false)
            } else {
                Err(Error::Conflict(
                    "identifiant de résultat réutilisé avec un autre contenu".into(),
                ))
            };
        }
        transaction.execute(
            "INSERT INTO study_results(project_id,id,network_version,kind,name,json)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                project.id.to_string(),
                result.id.to_string(),
                result.network_version.to_string(),
                format!("{:?}", result.kind),
                result.name,
                json
            ],
        )?;
        transaction.commit()?;
        Ok(true)
    }

    pub fn load_results(&self, project: &Project) -> Result<Vec<StoredResult>> {
        project.network()?;
        let mut statement = self
            .connection
            .prepare("SELECT json FROM study_results WHERE project_id=?1 ORDER BY rowid")?;
        let rows = statement.query_map([project.id.to_string()], |row| row.get::<_, String>(0))?;
        let mut results = vec![];
        for json in rows {
            let result: StoredResult = serde_json::from_str(&json?)?;
            result.validate_for(project)?;
            results.push(result);
        }
        Ok(results)
    }
}

fn save_project_transaction(
    transaction: &rusqlite::Transaction<'_>,
    project: &Project,
) -> Result<()> {
    let stored: Option<(u32, String, String)> = transaction
        .query_row(
            "SELECT schema_version,root_version,root_json FROM projects WHERE id=?1",
            [project.id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    if let Some((schema, root_version, root)) = stored {
        if ![1, 2, 3, 4, SCHEMA_VERSION].contains(&schema)
            || root_version != project.root_version.to_string()
            || serde_json::from_str::<crate::model::Network>(&root)? != project.root
        {
            return Err(Error::Conflict(
                "racine du projet différente de la base".into(),
            ));
        }
    } else {
        transaction.execute(
            "INSERT INTO projects VALUES (?1,?2,?3,?4,?5)",
            params![
                project.id.to_string(),
                project.schema_version,
                project.root_version.to_string(),
                serde_json::to_string(&project.root)?,
                project.root_version.to_string()
            ],
        )?;
    }
    let saved: Vec<String> = {
        let mut statement = transaction
            .prepare("SELECT json FROM changes WHERE project_id=?1 ORDER BY position")?;
        statement
            .query_map([project.id.to_string()], |row| row.get(0))?
            .collect::<std::result::Result<_, _>>()?
    };
    if saved.len() > project.changes.len() {
        return Err(Error::Conflict(
            "la base contient des versions plus récentes".into(),
        ));
    }
    for (position, json) in saved.iter().enumerate() {
        let change = serde_json::from_str::<ChangeSet>(json)?.migrate()?;
        if change != project.changes[position] {
            return Err(Error::Conflict("historique concurrent en base".into()));
        }
        let canonical = serde_json::to_string(&change)?;
        if canonical != *json {
            transaction.execute(
                "UPDATE changes SET json=?1 WHERE project_id=?2 AND position=?3",
                params![canonical, project.id.to_string(), position as i64],
            )?;
        }
    }
    for (position, change) in project.changes.iter().enumerate().skip(saved.len()) {
        transaction.execute(
            "INSERT INTO changes VALUES (?1,?2,?3,?4)",
            params![
                project.id.to_string(),
                position as i64,
                change.id.to_string(),
                serde_json::to_string(change)?
            ],
        )?;
    }
    transaction.execute(
        "UPDATE projects SET head=?1,schema_version=?3,root_json=?4 WHERE id=?2",
        params![
            project.head().to_string(),
            project.id.to_string(),
            project.schema_version,
            serde_json::to_string(&project.root)?
        ],
    )?;
    Ok(())
}

fn save_scenarios_transaction(
    transaction: &rusqlite::Transaction<'_>,
    project: &Project,
    json: &str,
) -> Result<()> {
    let stored_head: Option<String> = transaction
        .query_row(
            "SELECT head FROM projects WHERE id=?1",
            [project.id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    let head = project.head().to_string();
    if stored_head.as_deref() != Some(head.as_str()) {
        return Err(Error::Conflict(
            "enregistrer d'abord la même version du réseau avant ses scénarios".into(),
        ));
    }
    transaction.execute(
        "INSERT INTO scenario_collections(project_id,network_version,json) VALUES (?1,?2,?3)
         ON CONFLICT(project_id) DO UPDATE SET network_version=excluded.network_version,json=excluded.json",
        params![project.id.to_string(), head, json],
    )?;
    Ok(())
}
