use axum::{
    extract::{Path, State},
    http::{Method, StatusCode},
    response::{Json, Redirect},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::MySqlPool;
use std::time::Duration;
use tower_http::cors::{Any, CorsLayer};

// --- ESTRUCTURAS DEL MANIFEST Y STREMIO ---
#[derive(Serialize)]
struct Manifest {
    id: String,
    version: String,
    name: String,
    description: String,
    resources: Vec<String>,
    types: Vec<String>,
    id_prefixes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    behavior_hints: Option<BehaviorHints>,
}

#[derive(Serialize)]
struct BehaviorHints {
    configurable: bool,
    configuration_required: bool,
}

#[derive(Serialize)]
struct StreamBehaviorHints {
    #[serde(rename = "notSupported")]
    not_supported: bool,
}
#[derive(Serialize)]
struct StreamResponse {
    streams: Vec<StreamItem>,
}

#[derive(Serialize)]
struct StreamItem {
    name: String,
    title: String,
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    behavior_hints: Option<StreamBehaviorHints>,
}

// --- CLIENTE ALLDEBRID DINÁMICO ---
#[derive(Deserialize)]
struct AdAgentResponse<T> {
    status: String,
    data: Option<T>,
}

// 1. Respuesta de /magnet/upload
#[derive(Deserialize, Debug)]
struct AdUploadData {
    magnets: Vec<AdUploadMagnet>,
}

#[derive(Deserialize, Debug)]
struct AdUploadMagnet {
    id: Value,
    ready: bool,
}

// 2. Respuesta de /magnet/files
#[derive(Deserialize, Debug)]
struct AdFilesData {
    magnets: Vec<AdFilesMagnet>,
}

#[derive(Deserialize, Debug)]
struct AdFilesMagnet {
    #[allow(dead_code)]
    id: Value,
    files: Option<Vec<AdFileNode>>,
    error: Option<AdFilesError>,
}

#[derive(Deserialize, Debug)]
struct AdFilesError {
    #[allow(dead_code)]
    code: Option<String>,
    #[allow(dead_code)]
    message: Option<String>,
}

#[derive(Deserialize, Debug)]
struct AdFileNode {
    n: Option<String>,
    l: Option<String>,
    #[serde(default)]
    e: Vec<AdFileNode>,
}

// 3. Respuesta de /link/unlock
#[derive(Deserialize)]
struct AdUnlockData {
    link: String,
}

/// Helper para consultar la API de AllDebrid
async fn resolve_magnet_with_key(
    client: &reqwest::Client,
    api_key: &str,
    magnet: &str,
    _season: u32,
    episode: u32,
) -> Option<String> {
    // PASO 1: Subir / Registrar el Magnet
    let upload_url = format!(
        "https://api.alldebrid.com/v4/magnet/upload?agent=StremioAnimeIndexer&apikey={}&magnet={}",
        api_key,
        urlencoding::encode(magnet)
    );
    let upload_res: AdAgentResponse<AdUploadData> = client
        .get(&upload_url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;

    let upload_data = upload_res.data?;
    let magnet_info = upload_data.magnets.first()?;
    let magnet_id_str = match &magnet_info.id {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => magnet_info.id.to_string(),
    };

    println!("📊 Estado del magnet -> Ready: {}, ID: {}", magnet_info.ready, magnet_id_str);
    if !magnet_info.ready {
        eprintln!("⚠️ El magnet NO está disponible en la caché de AllDebrid.");
        return None;
    }

    // PASO 2: Obtener los archivos del Magnet (/magnet/files vía POST)
    println!("🔍 Consultando /magnet/files vía POST para id[]={}...", magnet_id_str);
    let files_url = format!(
        "https://api.alldebrid.com/v4/magnet/files?agent=StremioAnimeIndexer&apikey={}",
        api_key
    );

    let files_resp = client
        .post(&files_url)
        .form(&[("id[]", &magnet_id_str)])
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?;

    let body_text = files_resp.text().await.ok()?;
    let files_res: AdAgentResponse<AdFilesData> = match serde_json::from_str(&body_text) {
        Ok(parsed) => parsed,
        Err(e) => {
            eprintln!("❌ Error deserializando /magnet/files: {:?}", e);
            eprintln!("📄 Respuesta recibida: {}", body_text);
            return None;
        }
    };

    let files_data = files_res.data?;
    let files_magnet = files_data.magnets.first()?;

    if let Some(err) = &files_magnet.error {
        eprintln!("❌ Error de AllDebrid para este magnet: {:?}", err);
        return None;
    }

    let raw_files = files_magnet.files.as_ref()?;
    let mut all_files: Vec<(String, String)> = Vec::new();
    flatten_files(raw_files, &mut all_files);

    println!("📁 Archivos extraídos del torrent: {}", all_files.len());
    if all_files.is_empty() {
        eprintln!("⚠️ No se encontraron enlaces de archivos en la respuesta.");
        return None;
    }

    // Filtrar episodios y seleccionar el archivo de vídeo
    let ep_str_padded = format!("{:02}", episode);
    let ep_str_three = format!("{:03}", episode);
    let selected_file = all_files
        .iter()
        .find(|(name, _)| {
            let n = name.to_lowercase();
            let is_video = n.ends_with(".mkv") || n.ends_with(".mp4") || n.ends_with(".avi");
            if !is_video {
                return false;
            }
            n.contains(&format!("e{}", ep_str_padded))
                || n.contains(&format!("ep{}", ep_str_padded))
                || n.contains(&format!(" - {}", ep_str_padded))
                || n.contains(&format!(" - {}", ep_str_three))
                || n.contains(&format!(" {} ", ep_str_padded))
        })
        .or_else(|| {
            all_files.iter().find(|(name, _)| {
                let n = name.to_lowercase();
                n.ends_with(".mkv") || n.ends_with(".mp4") || n.ends_with(".avi")
            })
        })?;

    println!("🎬 Archivo seleccionado para reproducir: {}", selected_file.0);

    // PASO 3: Desbloquear el enlace web del archivo seleccionado
    unlock_link_with_key(client, api_key, &selected_file.1).await
}

/// Helper para aplanar carpetas anidadas en `/magnet/files`
fn flatten_files(nodes: &[AdFileNode], acc: &mut Vec<(String, String)>) {
    for node in nodes {
        if let (Some(name), Some(link)) = (&node.n, &node.l) {
            acc.push((name.clone(), link.clone()));
        }
        if !node.e.is_empty() {
            flatten_files(&node.e, acc);
        }
    }
}

async fn unlock_link_with_key(
    client: &reqwest::Client,
    api_key: &str,
    link: &str,
) -> Option<String> {
    let unlock_url = format!(
        "https://api.alldebrid.com/v4/link/unlock?agent=StremioAnimeIndexer&apikey={}&link={}",
        api_key,
        urlencoding::encode(link)
    );
    let res: AdAgentResponse<AdUnlockData> = client
        .get(&unlock_url)
        .timeout(Duration::from_secs(4))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;

    if res.status == "success" {
        let stream_url = res.data?.link;
        println!("🚀 Enlace final listo para Stremio: {}", stream_url);
        Some(stream_url)
    } else {
        eprintln!("❌ Error al desbloquear el enlace en AllDebrid");
        None
    }
}

// --- ESTADO COMPARTIDO DEL SERVIDOR ---
#[derive(Clone)]
pub struct AppState {
    pub db: MySqlPool,
    pub http_client: reqwest::Client,
}

// --- HANDLERS HTTP ---
async fn get_base_manifest() -> Json<Manifest> {
    Json(Manifest {
        id: "com.stremio.anime.alldebrid.indexer".to_string(),
        version: "1.0.0".to_string(),
        name: "Leydinime".to_string(),
        description: "Introduce tu API Key de AllDebrid en la URL para usar este addon."
            .to_string(),
        resources: vec![],
        types: vec!["series".to_string(), "movie".to_string()],
        id_prefixes: vec!["tt".to_string(), "kitsu".to_string()],
        behavior_hints: Some(BehaviorHints {
            configurable: true,
            configuration_required: true,
        }),
    })
}

async fn get_configured_manifest(Path(_api_key): Path<String>) -> Json<Manifest> {
    Json(Manifest {
        id: "com.stremio.anime.alldebrid.indexer".to_string(),
        version: "1.0.0".to_string(),
        name: "Leydinime".to_string(),
        description: "Servidor de anime indexado con reproducción via tu cuenta de AllDebrid."
            .to_string(),
        resources: vec!["stream".to_string()],
        types: vec!["series".to_string(), "movie".to_string()],
        id_prefixes: vec!["tt".to_string(), "kitsu".to_string()],
        behavior_hints: None,
    })
}

/// Endpoint `/stream/{type}/{id}.json`
async fn get_streams(
    Path((api_key, _type, id)): Path<(String, String, String)>,
    State(state): State<AppState>,
) -> Json<StreamResponse> {
    let raw_id = id.trim_end_matches(".json");
    let decoded_id = urlencoding::decode(raw_id)
        .unwrap_or_else(|_| raw_id.into())
        .into_owned();

    let parts: Vec<&str> = decoded_id.split(':').collect();
    let base_id = parts[0];
    let season: u32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
    let episode: u32 = parts.get(2).and_then(|e| e.parse().ok()).unwrap_or(1);

    let imdb_id = if base_id.starts_with("tt") { Some(base_id) } else { None };
    let mal_id = if !base_id.starts_with("tt") { base_id.parse::<u64>().ok() } else { None };

    // 1. Consultar MySQL solicitando info_hash
    let db_torrents = sqlx::query!(
        r#"
        SELECT t.title, t.info_hash, t.resolution, t.seeders, t.release_group
        FROM torrents t
        INNER JOIN animes a ON t.anime_id = a.mal_id
        WHERE (? IS NOT NULL AND a.imdb_id = ?)
           OR (? IS NOT NULL AND a.mal_id = ?)
        ORDER BY t.seeders DESC
        LIMIT 10
        "#,
        imdb_id, imdb_id, mal_id, mal_id
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let base_url = std::env::var("BASE_URL")
        .unwrap_or_else(|_| "https://stremio.lify.win".to_string());

    // 2. Construir la lista de streams usando t.info_hash
   let streams = db_torrents
       .into_iter()
       .filter_map(|t| {
           let hash = t.info_hash;
           let res_tag = t.resolution.unwrap_or_else(|| "SD".to_string());
           let group_tag = t.release_group.unwrap_or_else(|| "RAW".to_string());
           let seeders = t.seeders.unwrap_or(0);

           // URL limpia para resolver
           let stream_url = format!(
               "{}/resolve/{}/{}/{}/{}",
               base_url, api_key, season, episode, hash
           );

           // IMPORTANTE: Título en UNA SOLA LÍNEA sin '\n' para compatibilidad con la TV
           let clean_title = format!(
               "{} | Seeders: {} | Grupo: {}",
               t.title, seeders, group_tag
           );

           Some(StreamItem {
               name: format!("⚡ Leydinime [{}]", res_tag),
               title: clean_title,
               url: stream_url,
               behavior_hints: Some(StreamBehaviorHints {
                   not_supported: false,
               }),
           })
       })
   .collect();

    Json(StreamResponse { streams })
}

// --- HANDLER: /resolve por Path (Limpio para Smart TVs) ---
async fn resolve_stream_path(
    Path((key, season, episode, hash)): Path<(String, u32, u32, String)>,
    State(state): State<AppState>,
) -> Result<Redirect, (StatusCode, &'static str)> {
    let clean_magnet = format!("magnet:?xt=urn:btih:{}", hash);

    println!("🧲 Resolviendo Hash: {} para T{}:E{}", hash, season, episode);

    let stream_url = resolve_magnet_with_key(
        &state.http_client,
        &key,
        &clean_magnet,
        season,
        episode,
    )
        .await;

    match stream_url {
        Some(url) => {
            println!("✅ Redirigiendo Smart TV (HTTP 307) a: {}", url);
            Ok(Redirect::temporary(&url))
        }
        None => {
            eprintln!("❌ No se pudo resolver o no está listo en caché.");
            Err((
                    StatusCode::NOT_FOUND,
                    "El archivo no está en caché de AllDebrid",
            ))
        }
    }
}

// --- FUNCIÓN PRINCIPAL DE ARRANQUE ---
pub async fn run_server(pool: MySqlPool, port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .pool_max_idle_per_host(10)
        .build()?;

    let state = AppState {
        db: pool,
        http_client,
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(vec![Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(Any);

    let app = Router::new()
        .route("/manifest.json", get(get_base_manifest))
        .route("/{api_key}/manifest.json", get(get_configured_manifest))
        .route("/{api_key}/stream/{media_type}/{id}", get(get_streams))
        // Nueva ruta limpia de resolución
        .route("/resolve/{key}/{season}/{episode}/{hash}", get(resolve_stream_path))
        .layer(cors)
        .with_state(state);

    let bind_host = std::env::var("BIND_HOST")
        .unwrap_or_else(|_| "0.0.0.0".to_string());
    let addr = format!("{}:{}", bind_host, port);
    println!("🚀 Servidor Stremio optimizado escuchando en {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
