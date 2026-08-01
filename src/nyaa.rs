use regex::Regex;
use rss::Channel;
use sqlx::MySqlPool;
use std::error::Error;
use std::time::Duration;
use tokio::time::sleep;

#[derive(Debug)]
pub struct TorrentRecord {
    pub anime_id: u64,
    pub title: String,
    pub info_hash: String,
    pub magnet: String,
    pub episode: Option<i32>,
    pub resolution: Option<String>,
    pub release_group: Option<String>,
    pub seeders: i32,
    pub leechers: i32,
}

/// Extrae el grupo de release (ej. [SubsPlease], [Erai-raws]), 
/// el episodio (ej. E01 u 01) y la resolución (1080p, 720p).
use std::sync::OnceLock;

fn parse_torrent_title(title: &str) -> (Option<String>, Option<i32>, Option<String>) {
    static RE_GROUP: OnceLock<Regex> = OnceLock::new();
    static RE_RES: OnceLock<Regex> = OnceLock::new();
    static RE_EP: OnceLock<Regex> = OnceLock::new();

    let re_group = RE_GROUP.get_or_init(|| Regex::new(r"^\[([^\\]]+)\]").unwrap());
    let re_resolution = RE_RES.get_or_init(|| Regex::new(r"(1080p|720p|480p|2160p)").unwrap());
    let re_episode = RE_EP.get_or_init(|| Regex::new(r"(?:[eE]| - )(\d{1,4})(?:v\d)?\b").unwrap());

    let release_group = re_group.captures(title).map(|c| c.get(1).unwrap().as_str().to_string());
    let resolution = re_resolution.captures(title).map(|c| c.get(1).unwrap().as_str().to_string());
    let episode = re_episode.captures(title).and_then(|c| c.get(1).unwrap().as_str().parse::<i32>().ok());

    (release_group, episode, resolution)
}

// Si el enlace es magnet:?xt=urn:btih:HASH...
fn extract_info_hash(url: &str) -> Option<String> {
    static RE_HASH: OnceLock<Regex> = OnceLock::new();
    let re_hash = RE_HASH.get_or_init(|| Regex::new(r"(?i)(?:urn:btih:|download/)([a-f0-9]{40})").unwrap());
    
    re_hash.captures(url).map(|c| c.get(1).unwrap().as_str().to_lowercase())
}

pub async fn scrape_nyaa_for_anime(
    client: &reqwest::Client,
    anime_id: u64,
    query_title: &str,
) -> Result<Vec<TorrentRecord>, Box<dyn Error>> {
    let encoded_query = urlencoding::encode(query_title);
    let rss_url = format!(
        "https://nyaa.si/?page=rss&q={}&c=0_0&f=0",
        encoded_query
    );
    let content = client.get(&rss_url).send().await?.bytes().await?;
    let channel = Channel::read_from(&content[..])?;

    let mut records = Vec::new();

    for item in channel.items() {
        let title = match item.title() {
            Some(t) => t.to_string(),
            None => continue,
        };

        // 1. Obtener InfoHash y Seeders/Leechers de las extensiones XML <nyaa:*>
        let mut info_hash = None;
        let mut seeders = 0;
        let mut leechers = 0;

        if let Some(nyaa_ext) = item.extensions().get("nyaa") {
            if let Some(hash_vec) = nyaa_ext.get("infoHash") {
                if let Some(first) = hash_vec.first() {
                    info_hash = first.value().map(|h| h.to_lowercase());
                }
            }
            if let Some(s_vec) = nyaa_ext.get("seeders") {
                if let Some(first) = s_vec.first() {
                    seeders = first.value().unwrap_or("0").parse::<i32>().unwrap_or(0);
                }
            }
            if let Some(l_vec) = nyaa_ext.get("leechers") {
                if let Some(first) = l_vec.first() {
                    leechers = first.value().unwrap_or("0").parse::<i32>().unwrap_or(0);
                }
            }
        }

        // Si no se encuentra en las extensiones, intentamos extraerlo de la descripción
        let hash = match info_hash {
            Some(h) => h,
            None => match item.description().and_then(extract_info_hash) {
                Some(h) => h,
                None => continue, // Si realmente no hay hash, ignorar
            },
        };

        // 2. Generar el enlace Magnet real para Stremio / AllDebrid
        let magnet_link = format!(
            "magnet:?xt=urn:btih:{}&dn={}",
            hash,
            urlencoding::encode(&title)
        );

        let (release_group, episode, resolution) = parse_torrent_title(&title);

        records.push(TorrentRecord {
            anime_id,
            title,
            info_hash: hash,
            magnet: magnet_link,
            episode,
            resolution,
            release_group,
            seeders,
            leechers,
        });
    }

    Ok(records)
}

/// Sincroniza torrents desde Nyaa.si para todos los animes en MySQL
pub async fn sync_nyaa_torrents_full(pool: &MySqlPool) -> Result<(), Box<dyn Error>> {
    println!("🚀 Iniciando escaneo NOCTURNO COMPLETO de Nyaa.si...");

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) StremioIndexer/1.0")
        .timeout(Duration::from_secs(15))
        .build()?;

    let batch_size = 50;
    let mut total_processed = 0;

    loop {
        // Obtenemos los animes que NUNCA se han escrapeado o tienen el scraping más antiguo
        let rows = sqlx::query!(
            r#"
            SELECT mal_id, title 
            FROM animes 
            WHERE status != 'Not yet aired'
            ORDER BY last_scraped_at IS NOT NULL, last_scraped_at ASC 
            LIMIT ?
            "#,
            batch_size
        )
            .fetch_all(pool)
            .await?;

        if rows.is_empty() {
            println!("✅ Todos los animes han sido procesados.");
            break;
        }

        for row in &rows {
            total_processed += 1;
            println!(
                "[#{}] Buscando torrents para: {} (ID: {})",
                total_processed, row.title, row.mal_id
            );

            let mut retries = 0;
            let max_retries = 3;

            while retries < max_retries {
                match scrape_nyaa_for_anime(&client, row.mal_id, &row.title).await {
                    Ok(torrents) => {
                        let mut tx = pool.begin().await?;

                        for t in torrents {
                            sqlx::query!(
                                r#"
                                INSERT INTO torrents (
                                    anime_id, title, info_hash, magnet, episode, 
                                    resolution, release_group, seeders, leechers
                                )
                                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                                ON DUPLICATE KEY UPDATE
                                    seeders = VALUES(seeders),
                                    leechers = VALUES(leechers)
                                "#,
                                t.anime_id, t.title, t.info_hash, t.magnet,
                                t.episode, t.resolution, t.release_group,
                                t.seeders, t.leechers
                            )
                                .execute(&mut *tx)
                                .await?;
                            }

                        // Actualizar la marca de tiempo de escaneo
                        sqlx::query!(
                            "UPDATE animes SET last_scraped_at = CURRENT_TIMESTAMP WHERE mal_id = ?",
                            row.mal_id
                        )
                            .execute(&mut *tx)
                            .await?;

                        tx.commit().await?;
                        break; // Éxito, salir del bucle de reintentos
                    }
                    Err(e) => {
                        retries += 1;
                        eprintln!(
                            "⚠️ Error en '{}' (Intento {}/{}): {}",
                            row.title, retries, max_retries, e
                        );
                        // Espera más larga si falla (posible rate-limit o caída de Nyaa)
                        sleep(Duration::from_secs(5 * retries as u64)).await;
                    }
                }
            }

            // Pausa de 2 segundos entre peticiones para mantener el proceso seguro durante la noche
            sleep(Duration::from_millis(2000)).await;
        }

        println!("--- Lote de {} animes completado. Guardando avance... ---", rows.len());
    }

    println!("🎉 Proceso nocturno finalizado. Total animes revisados: {}", total_processed);
    Ok(())
}
