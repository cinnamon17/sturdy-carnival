use axum::{
    extract::{Path, Query, State},
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
struct StreamResponse {
    streams: Vec<StreamItem>,
}

#[derive(Serialize)]
struct StreamItem {
    name: String,
    title: String,
    url: String,
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
    id: Value, // Puede venir como int o string ("123" o 123)
    files: Option<Vec<AdFileNode>>, // Usamos Option porque si hay error no existe 'files'
    error: Option<AdFilesError>,    // Captura errores tipo MAGNET_INVALID_ID
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
    n: Option<String>,          // Nombre del archivo o carpeta
    l: Option<String>,          // URL web para unlock (solo presente en archivos)
    #[serde(default)]
    e: Vec<AdFileNode>,         // Subarchivos o subcarpetas
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
    // -------------------------------------------------------------
    // PASO 1: Subir / Registrar el Magnet
    // -------------------------------------------------------------
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

    // -------------------------------------------------------------
    // PASO 2: Obtener los archivos del Magnet (/magnet/files vía POST)
    // -------------------------------------------------------------
    println!("🔍 2. Consultando /magnet/files vía POST para id[]={}...", magnet_id_str);

    let files_url = format!(
        "https://api.alldebrid.com/v4/magnet/files?agent=StremioAnimeIndexer&apikey={}",
        api_key
    );

    // Enviamos como Form POST con la clave `id[]` exacta que pide la API
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

    // Verificar si AllDebrid devolvió un error para este ID
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

    // -------------------------------------------------------------
    // PASO 3: Desbloquear el enlace web del archivo seleccionado
    // -------------------------------------------------------------
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

/// Endpoint `/stream/{type}/{id}.json` optimizado con peticiones en PARALELO
async fn get_streams(
    Path((api_key, _type, id)): Path<(String, String, String)>,
    State(state): State<AppState>,
) -> Json<StreamResponse> {
    let clean_id = id.trim_end_matches(".json");

    // Extraer ID y posibles parámetros de temporada/episodio (ej. tt0434693:1:1)
    let parts: Vec<&str> = clean_id.split(':').collect();
    let base_id = parts[0];
    let season: u32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
    let episode: u32 = parts.get(2).and_then(|e| e.parse().ok()).unwrap_or(1);

    let imdb_id = if base_id.starts_with("tt") {
        Some(base_id)
    } else {
        None
    };
    let mal_id = if !base_id.starts_with("tt") {
        base_id.parse::<u64>().ok()
    } else {
        None
    };

    // 1. Solo consultamos MySQL
    let db_torrents = sqlx::query!(
        r#"
        SELECT t.title, t.magnet, t.resolution, t.seeders, t.release_group
        FROM torrents t
        INNER JOIN animes a ON t.anime_id = a.mal_id
        WHERE (? IS NOT NULL AND a.imdb_id = ?)
           OR (? IS NOT NULL AND a.mal_id = ?)
        ORDER BY t.seeders DESC
        LIMIT 10
        "#,
        imdb_id,
        imdb_id,
        mal_id,
        mal_id
    )
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

    // 2. Construimos la lista INMEDIATAMENTE sin llamar a AllDebrid aún
    let streams = db_torrents
        .into_iter()
        .map(|t| {
            let res_tag = t.resolution.unwrap_or_else(|| "SD".to_string());
            let group_tag = t.release_group.unwrap_or_else(|| "RAW".to_string());

            // Encodeamos el magnet para pasarlo seguro por URL
            let encoded_magnet = urlencoding::encode(&t.magnet);

            // Añadimos season y episode a la URL proxy
            let stream_url = format!(
                "http://localhost:7000/resolve?key={}&season={}&episode={}&magnet={}",
                api_key, season, episode, encoded_magnet
            );

            StreamItem {
                name: format!("⚡ [Leydinime] [{}]", res_tag),
                title: format!(
                    "{}\n👥 Seeders: {} | 👥 Grupo: {}",
                    t.title,
                    t.seeders.unwrap_or(0),
                    group_tag
                ),
                url: stream_url,
            }
        })
    .collect();

    Json(StreamResponse { streams })
}

// --- HANDLER: /resolve (Se ejecuta SOLO al pulsar Play) ---
#[derive(Deserialize)]
struct ResolveParams {
    key: String,
    season: Option<u32>,
    episode: Option<u32>,
    magnet: String,
}

async fn resolve_stream(
    Query(params): Query<ResolveParams>,
    State(state): State<AppState>,
) -> Result<Redirect, (StatusCode, &'static str)> {
    let first_decode = urlencoding::decode(&params.magnet)
        .unwrap_or_else(|_| params.magnet.clone().into())
        .into_owned();

    let clean_magnet = if first_decode.contains("%25") {
        urlencoding::decode(&first_decode)
            .unwrap_or_else(|_| first_decode.clone().into())
            .into_owned()
    } else {
        first_decode
    };

    println!("🧲 Magnet final procesado: {}", clean_magnet);

    // Intentar resolver el magnet en AllDebrid
    let stream_url = resolve_magnet_with_key(
        &state.http_client,
        &params.key,
        &clean_magnet,
        params.season.unwrap_or(1),
        params.episode.unwrap_or(1),
    )
        .await;

    match stream_url {
        Some(url) => {
            println!("✅ Redirigiendo a enlace directo: {}", url);
            Ok(Redirect::to(&url))
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
        .allow_methods(vec![Method::GET])
        .allow_headers(Any);

    let app = Router::new()
        .route("/manifest.json", get(get_base_manifest))
        .route("/{api_key}/manifest.json", get(get_configured_manifest))
        .route("/{api_key}/stream/{media_type}/{id}", get(get_streams))
        .route("/resolve", get(resolve_stream))
        .layer(cors)
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    println!("🚀 Servidor Stremio optimizado escuchando en http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
