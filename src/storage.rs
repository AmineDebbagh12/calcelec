use crate::{
    model::{Error, Id, Result},
    project::{ChangeSet, Project, SCHEMA_VERSION},
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
                PRIMARY KEY(project_id, position), UNIQUE(project_id, id));",
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
        let stored: Option<(u32, String, String)> = transaction
            .query_row(
                "SELECT schema_version,root_version,root_json FROM projects WHERE id=?1",
                [project.id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((schema, root_version, root)) = stored {
            if ![1, 2, 3, SCHEMA_VERSION].contains(&schema)
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
            let mut stmt = transaction
                .prepare("SELECT json FROM changes WHERE project_id=?1 ORDER BY position")?;
            stmt.query_map([project.id.to_string()], |r| r.get(0))?
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
        transaction.commit()?;
        Ok(())
    }
}
