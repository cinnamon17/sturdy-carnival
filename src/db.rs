use sqlx::MySqlPool;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};

/// Normaliza fechas incompletas de MAL a formato YYYY-MM-DD para MySQL
fn parse_mal_date(date_str: Option<&str>) -> Option<String> {
    let s = date_str?.trim();
    if s.is_empty() {
        return None;
    }

    let parts: Vec<&str> = s.split('-').collect();
    match parts.len() {
        // "2003" -> "2003-01-01"
        1 => {
            if parts[0].len() == 4 {
                Some(format!("{}-01-01", parts[0]))
            } else {
                None
            }
        }
        // "2003-04" -> "2003-04-01"
        2 => {
            if parts[0].len() == 4 && parts[1].len() <= 2 {
                Some(format!("{}-{:0>2}-01", parts[0], parts[1]))
            } else {
                None
            }
        }
        // "2003-04-07" -> "2003-04-07"
        3 => Some(s.to_string()),
        _ => None,
    }
}

pub async fn sync_jsonl_to_db(
    pool: &MySqlPool,
    jsonl_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Iniciando sincronización con la base de datos MySQL...");

    let file = File::open(jsonl_path).await?;
    let mut reader = BufReader::new(file).lines();

    let mut count = 0;
    let batch_size = 1000;

    // Iniciamos la primera transacción
    let mut tx = pool.begin().await?;

    while let Some(line) = reader.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }

        let anime: serde_json::Value = serde_json::from_str(&line)?;

        let mal_id = anime["id"].as_u64().unwrap_or(0);
        let title = anime["title"].as_str().unwrap_or("");
        let synopsis = anime["synopsis"].as_str();
        let media_type = anime["media_type"].as_str();
        let status = anime["status"].as_str();
        let num_episodes = anime["num_episodes"].as_i64();
        let score = anime["mean"].as_f64();
        let rank_pos = anime["rank"].as_i64();
        let popularity = anime["popularity"].as_i64();
        let num_list_users = anime["num_list_users"].as_u64();
        let num_scoring_users = anime["num_scoring_users"].as_u64();
        let nsfw = anime["nsfw"].as_str();
        let rating = anime["rating"].as_str();
        let source = anime["source"].as_str();
        let start_date_raw = anime["start_date"].as_str();
        let start_date = parse_mal_date(start_date_raw);

        let end_date_raw = anime["end_date"].as_str();
        let end_date = parse_mal_date(end_date_raw);

        let imdb_id = anime["imdb_id"].as_str();
        let kitsu_id = anime["kitsu_id"].as_u64();
        let anilist_id = anime["anilist_id"].as_u64();

        let main_picture = anime.get("main_picture").map(|v| v.to_string());
        let alternative_titles = anime.get("alternative_titles").map(|v| v.to_string());
        let genres = anime.get("genres").map(|v| v.to_string());
        let studios = anime.get("studios").map(|v| v.to_string());

        // Insertamos usando la transacción actual (&mut *tx)
        sqlx::query!(
            r#"
            INSERT INTO animes (
                mal_id, title, synopsis, media_type, status, num_episodes, 
                start_date, end_date, score, rank_pos, popularity, 
                num_list_users, num_scoring_users, nsfw, rating, source,
                imdb_id, kitsu_id, anilist_id, main_picture, alternative_titles, 
                genres, studios
            )
            VALUES (
                ?, ?, ?, ?, ?, ?, 
                ?, ?, ?, ?, ?, 
                ?, ?, ?, ?, ?,
                ?, ?, ?, ?, ?, 
                ?, ?
            )
            ON DUPLICATE KEY UPDATE
                title = VALUES(title),
                synopsis = VALUES(synopsis),
                media_type = VALUES(media_type),
                status = VALUES(status),
                num_episodes = VALUES(num_episodes),
                start_date = VALUES(start_date),
                end_date = VALUES(end_date),
                score = VALUES(score),
                rank_pos = VALUES(rank_pos),
                popularity = VALUES(popularity),
                num_list_users = VALUES(num_list_users),
                num_scoring_users = VALUES(num_scoring_users),
                nsfw = VALUES(nsfw),
                rating = VALUES(rating),
                source = VALUES(source),
                imdb_id = VALUES(imdb_id),
                kitsu_id = VALUES(kitsu_id),
                anilist_id = VALUES(anilist_id),
                main_picture = VALUES(main_picture),
                alternative_titles = VALUES(alternative_titles),
                genres = VALUES(genres),
                studios = VALUES(studios)
            "#,
            mal_id, title, synopsis, media_type, status, num_episodes,
            start_date, end_date, score, rank_pos, popularity,
            num_list_users, num_scoring_users, nsfw, rating, source,
            imdb_id, kitsu_id, anilist_id, main_picture, alternative_titles,
            genres, studios
                )
                .execute(&mut *tx)
                .await?;

        count += 1;

        // Cada 1000 registros guardamos la transacción en MySQL y abrimos una nueva
        if count % batch_size == 0 {
            tx.commit().await?;
            tx = pool.begin().await?;
            println!("Procesados {} registros...", count);
        }
    }

    // Confirmamos los registros restantes que no completaron un lote completo
    tx.commit().await?;

    println!("¡Base de datos actualizada con éxito! Total: {} animes.", count);
    Ok(())
}
