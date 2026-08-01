use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Debug, Deserialize)]
struct FribbItem {
    mal_id: Option<u64>,
    imdb_id: Option<Vec<String>>,
    kitsu_id: Option<u64>,
    anilist_id: Option<u64>,
}

/// Ejecuta el proceso de enriquecimiento descargando los datos desde `fribb_url`
/// y vinculando con `input_path` para guardarlos en `output_path`.
pub async fn enrich_anime_data(
    fribb_url: &str,
    input_path: &str,
    output_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Descargando y cargando mapping de Fribb desde la URL...");

    // 1. Petición HTTP para obtener la lista de Fribb directamente desde internet
    let fribb_list: Vec<FribbItem> = reqwest::get(fribb_url)
        .await?
        .json()
        .await?;

    let mut fribb_map: HashMap<u64, FribbItem> = HashMap::new();
    for item in fribb_list {
        if let Some(mal_id) = item.mal_id {
            fribb_map.insert(mal_id, item);
        }
    }

    println!("Mapping cargado: {} animes indexados.", fribb_map.len());

    // Lectura asíncrona del archivo de entrada
    let input_file = File::open(input_path).await?;
    let mut reader = BufReader::new(input_file).lines();

    let mut output_file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true) // Vacía el archivo si ya existe
        .open(output_path)
        .await?;

    let mut total = 0;
    let mut con_imdb = 0;

    println!("Procesando y enriqueciendo registros...");

    // Bucle asíncrono para leer línea por línea
    while let Some(line) = reader.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }

        let mut anime: Value = serde_json::from_str(&line)?;
        total += 1;

        if let Some(mal_id) = anime.get("id").and_then(|v| v.as_u64()) {
            if let Some(fribb) = fribb_map.get(&mal_id) {
                if let Some(imdb_list) = &fribb.imdb_id {
                    if let Some(first_imdb) = imdb_list.first() {
                        anime["imdb_id"] = Value::String(first_imdb.clone());
                        con_imdb += 1;
                    } else {
                        anime["imdb_id"] = Value::Null;
                    }
                } else {
                    anime["imdb_id"] = Value::Null;
                }

                anime["kitsu_id"] = fribb.kitsu_id.map_or(Value::Null, |id| Value::Number(id.into()));
                anime["anilist_id"] = fribb.anilist_id.map_or(Value::Null, |id| Value::Number(id.into()));
            } else {
                anime["imdb_id"] = Value::Null;
                anime["kitsu_id"] = Value::Null;
                anime["anilist_id"] = Value::Null;
            }
        }

        let json_line = serde_json::to_string(&anime)?;
        output_file.write_all(json_line.as_bytes()).await?;
        output_file.write_all(b"\n").await?;
    }

    output_file.flush().await?;

    println!("\n¡Enriquecimiento completado!");
    println!("Total procesados: {}", total);
    println!("Total con IMDb ID: {}", con_imdb);

    Ok(())
}
