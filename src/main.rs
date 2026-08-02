mod db;
mod enrich;
mod nyaa;
mod scraper;
mod server;

use clap::{Parser, Subcommand};
use sqlx::mysql::MySqlPoolOptions;
use sqlx::MySqlPool;
use std::env;

#[derive(Parser)]
#[command(name = "Stremio Anime Indexer")]
#[command(about = "Pipeline de extracción y enriquecimiento de animes", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Ejecuta todo el pipeline completo (ideal para cron nocturno)
    All,
    /// PASO 1: Descarga el ranking de MyAnimeList
    Mal,
    /// PASO 2: Enriquece las IDs con datos de Fribb
    Enrich,
    /// PASO 3: Sincroniza el archivo JSONL enriquecido en MySQL
    DbSync,
    /// PASO 4: Indexa torrents desde Nyaa.si hacia MySQL
    Nyaa,
    /// Inicializacion del server
    Serve,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    let args = Cli::parse();

    match args.command {
        Commands::All => {
            println!("🚀 Ejecutando PIPELINE COMPLETO...");
            run_mal().await?;
            run_enrich().await?;
            let pool = init_db_pool().await?;
            run_db_sync(&pool).await?;
            run_nyaa(&pool).await?;
            println!("\n🎉 ¡Pipeline completo finalizado exitosamente!");
        }
        Commands::Mal => {
            run_mal().await?;
        }
        Commands::Enrich => {
            run_enrich().await?;
        }
        Commands::DbSync => {
            let pool = init_db_pool().await?;
            run_db_sync(&pool).await?;
        }
        Commands::Nyaa => {
            let pool = init_db_pool().await?;
            run_nyaa(&pool).await?;
        }
        Commands::Serve => {
            let pool = init_db_pool().await?;
            let port = env::var("PORT")
                .unwrap_or_else(|_| "7000".to_string())
                .parse::<u16>()?;

            server::run_server(pool, port).await?;
        }
    }

    Ok(())
}

// Helper para conectar a la Base de Datos solo cuando sea necesario
async fn init_db_pool() -> Result<MySqlPool, Box<dyn std::error::Error>> {
    let db_url = env::var("DATABASE_URL")
        .expect("La variable DATABASE_URL debe estar definida en el archivo .env");

    let pool = MySqlPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await?;

    Ok(pool)
}

// Funciones wrapper por paso para mantener el código limpio y reusable

async fn run_mal() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== PASO 1: Descargando datos de MyAnimeList ===");
    scraper::fetch_mal_ranking().await
}

async fn run_enrich() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== PASO 2: Enriqueciendo datos con Fribb ===");
    enrich::enrich_anime_data(
        "https://raw.githubusercontent.com/Fribb/anime-lists/refs/heads/master/anime-list-full.json",
        "animes_dump.jsonl",
        "animes_enriched.jsonl",
    )
        .await
}

async fn run_db_sync(pool: &MySqlPool) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== PASO 3: Guardando en MySQL ===");
    db::sync_jsonl_to_db(pool, "animes_enriched.jsonl").await
}

async fn run_nyaa(pool: &MySqlPool) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== PASO 4: Indexando Torrents desde Nyaa.si ===");
    nyaa::sync_nyaa_torrents_full(pool).await
}
