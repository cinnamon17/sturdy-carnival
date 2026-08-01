use serde::{Deserialize, Serialize};
use std::env;
#[derive(Debug, Serialize, Deserialize)]
pub struct RankingResponse {
    pub data: Vec<AnimeNodeContainer>,
    pub paging: Paging,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnimeNodeContainer {
    pub node: AnimeNode,
    pub ranking: Option<RankingInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RankingInfo {
    pub rank: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnimeNode {
    pub id: u64,
    pub title: String,
    pub main_picture: Option<Picture>,
    pub alternative_titles: Option<AlternativeTitles>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub synopsis: Option<String>,
    pub mean: Option<f64>,
    pub rank: Option<i32>,
    pub popularity: Option<i32>,
    pub num_list_users: Option<u64>,
    pub num_scoring_users: Option<u64>,
    pub nsfw: Option<String>,
    pub media_type: Option<String>,
    pub status: Option<String>,
    pub genres: Option<Vec<Genre>>,
    pub num_episodes: Option<u32>,
    pub source: Option<String>,
    pub rating: Option<String>,
    pub studios: Option<Vec<Studio>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Picture {
    pub medium: String,
    pub large: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AlternativeTitles {
    pub synonyms: Option<Vec<String>>,
    pub en: Option<String>,
    pub ja: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Genre {
    pub id: u32,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Studio {
    pub id: u32,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Paging {
    pub next: Option<String>,
    pub previous: Option<String>,
}
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::Client;
use std::time::Duration;
use tokio::time::sleep;
use tokio::fs::OpenOptions;       
use tokio::io::AsyncWriteExt;

// 1. Cargar las variables del archivo .env

pub async fn fetch_mal_ranking() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Configuración de headers
    dotenvy::dotenv().ok();
    // Sustituye con tu Client ID real de MyAnimeList
    let mal_client_id = env::var("MAL_CLIENT_ID")
        .expect("el client id de my anime list debe estar informado en el .env");

    let mut headers = HeaderMap::new();
    // Convertimos la String dinámica a HeaderValue usando TryFrom / HeaderValue::try_from
    headers.insert(
        "X-MAL-CLIENT-ID", 
        HeaderValue::try_from(mal_client_id).expect("MAL_CLIENT_ID contiene caracteres inválidos para un header HTTP")
    );

    let client = Client::builder()
        .default_headers(headers)
        .build()?;


    // Abrimos/creamos el archivo en modo append
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true) // Vacía el archivo si ya existe
        .open("animes_dump.jsonl") // u "animes_enriched.jsonl"
        .await?;

    // 2. URL inicial (puedes ajustar el limit hasta 500 por petición según la API)
    let mut current_url: Option<String> = Some(
        "https://api.myanimelist.net/v2/anime/ranking?ranking_type=all&limit=500&nsfw=true&fields=id,title,main_picture,alternative_titles,start_date,end_date,synopsis,mean,rank,popularity,num_list_users,num_scoring_users,nsfw,created_at,updated_at,media_type,status,genres,num_episodes,start_season,broadcast,source,average_episode_duration,rating,pictures,background,related_anime,related_manga,recommendations,studios,statistics".to_string()
    );

    let mut total_fetched = 0;

    // 3. Bucle de extracción de datos con manejo de reintentos
    while let Some(url) = current_url {
        println!("Obteniendo datos desde: {}", url);

        let max_retries = 3;
        let mut retry_count = 0;
        let mut response_opt = None;

        // Bucle de reintentos para la URL actual
        while retry_count < max_retries {
            match client.get(&url).send().await {
                Ok(res) if res.status().is_success() => {
                    response_opt = Some(res);
                    break; // Petición exitosa, salimos del bucle de reintentos
                }
                Ok(res) if res.status().as_u16() == 429 => {
                    retry_count += 1;
                    eprintln!(
                        "⚠️ Límite de peticiones alcanzado (429). Reintento {}/{} en 5 segundos...",
                        retry_count, max_retries
                    );
                    sleep(Duration::from_secs(5)).await;
                }
                Ok(res) => {
                    retry_count += 1;
                    eprintln!(
                        "⚠️ Error HTTP {}. Reintento {}/{} en 2 segundos...",
                        res.status(), retry_count, max_retries
                    );
                    sleep(Duration::from_secs(2)).await;
                }
                Err(err) => {
                    retry_count += 1;
                    eprintln!(
                        "⚠️ Error de conexión: {}. Reintento {}/{} en 3 segundos...",
                        err, retry_count, max_retries
                    );
                    sleep(Duration::from_secs(3)).await;
                }
            }
        }

        // Si fallaron todos los reintentos, cancelamos el proceso ordenadamente
        let response = match response_opt {
            Some(res) => res,
            None => {
                eprintln!("❌ No se pudo recuperar la página tras {} intentos. Abortando scraper.", max_retries);
                break;
            }
        };

        // Parseo de la respuesta JSON
        let ranking_page: RankingResponse = match response.json().await {
            Ok(page) => page,
            Err(err) => {
                eprintln!("❌ Error al parsear JSON devuelto por MAL: {}", err);
                break;
            }
        };

        // Guardado en el JSONL
        for item in &ranking_page.data {
            total_fetched += 1;
            println!("[#{}] Guardando ID: {} | Título: {}", total_fetched, item.node.id, item.node.title);

            let json_line = serde_json::to_string(&item.node)?;
            file.write_all(json_line.as_bytes()).await?;
            file.write_all(b"\n").await?;
        }

        file.flush().await?;
        current_url = ranking_page.paging.next;

        // Pausa habitual de respetabilidad entre peticiones exitosas
        sleep(Duration::from_millis(500)).await;
    }

    println!("\n¡Extracción completada! Se guardaron {} animes en 'animes_dump.jsonl'.", total_fetched);
    Ok(())
}
