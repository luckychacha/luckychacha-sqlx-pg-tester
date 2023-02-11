use sqlx::{migrate::Migrator, Connection, Executor, PgPool};
use std::{path::Path, thread};

use sqlx::PgConnection;
use tokio::runtime::Runtime;
use uuid::Uuid;

pub struct TestDb {
    pub dbname: String,
    pub user: String,
    pub password: String,
    pub host: String,
    pub port: u16,
}

impl TestDb {
    pub fn new(
        user: impl Into<String>,
        password: impl Into<String>,
        host: impl Into<String>,
        port: u16,
        migration_path: impl Into<String>,
    ) -> Self {
        let uuid = Uuid::new_v4();
        let dbname = format!("test_{uuid}");
        let dbname_clone = dbname.clone();

        let user = user.into();
        let password = password.into();
        let host = host.into();
        let migration_path = migration_path.into();

        let tdb = Self {
            dbname,
            user,
            password,
            host,
            port,
        };

        let server_url = tdb.server_url();
        let db_url = tdb.url();

        thread::spawn(move || {
            let rt = Runtime::new().unwrap();
            rt.block_on(async move {
                let mut conn = PgConnection::connect(&server_url).await.unwrap();
                conn.execute(format!(r#"CREATE DATABASE "{dbname_clone}""#).as_str())
                    .await
                    .expect("Failed when create database {dbname_clone}.");

                let mut conn = PgConnection::connect(&db_url).await.unwrap();

                let m = Migrator::new(Path::new(&migration_path)).await.unwrap();
                m.run(&mut conn).await.unwrap();
            })
        })
        .join()
        .expect("Create database failed.");

        tdb
    }

    pub fn server_url(&self) -> String {
        if self.password.is_empty() {
            format!("postgres://{}@{}:{}", self.user, self.host, self.port)
        } else {
            format!(
                "postgres://{}:{}@{}:{}",
                self.user, self.password, self.host, self.port
            )
        }
    }

    pub fn url(&self) -> String {
        format!("{}/{}", self.server_url(), self.dbname)
    }

    pub async fn get_pool(&self) -> PgPool {
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .connect(&self.url())
            .await
            .unwrap()
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let server_url = self.server_url();
        let database_name = self.dbname.clone();
        thread::spawn(move || {
            let rt = Runtime::new().unwrap();
            rt.block_on(async move {
                let mut conn = PgConnection::connect(&server_url).await.unwrap();

                #[allow(clippy::expect_used)]
                sqlx::query(&format!(r#"SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE pid <> pg_backend_pid() AND datname = '{database_name}'"#))
                    .execute(&mut conn)
                    .await
                    .expect("Terminate all other connections");
                #[allow(clippy::expect_used)]
                sqlx::query(&format!(r#"DROP DATABASE "{database_name}""#))
                    .execute(&mut conn)
                    .await
                    .expect("Deleting the database");
            })
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_db_should_create_and_drop() {
        let tdb = TestDb::new("postgres", "postgres", "localhost", 5432, "./migrations");
        let pool = tdb.get_pool().await;
        sqlx::query("INSERT INTO todos(title) VALUES ('test')")
            .execute(&pool)
            .await
            .unwrap();
        let (id, title) = sqlx::query_as::<_, (i32, String)>("SELECT id, title FROM todos")
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(id, 1);
        assert_eq!(title, "test");
    }
}
