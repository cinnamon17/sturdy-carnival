use serde::Deserialize;
use std::error::Error;

#[derive(Debug, Deserialize)]
struct Category {
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Anime {
    pub id: String,
    pub title: String,
    pub synopsis: String,
    pub slug: String,
    pub category: Option<Category>, 
}

pub fn extraer_catalogo() -> Result<(), Box<dyn Error>> {
    let letters = vec!["0","A","B","C","D","E","F","G","H","I","J","K","L","M","N","O","P","Q","R","S","T","U","V","W","X","Y","Z"];
    let mut wtr = csv::Writer::from_path("catalogo_animes.csv")?;
    wtr.write_record(&["id", "titulo", "slug", "sinopsis", "categoria"])?;

    for letter in letters {

        let url = format!("https://animeav1.com/catalogo?letter={}",letter);
        let body = reqwest::blocking::get(url)?.text()?;

        let start_pat = "results:[";
        let end_pat = "],total:";

        let mut total_pages = 1;
        if let Some(pos) = body.find("totalPages:") {
            let start_num = pos + 11;
            if let Some(end_num) = body[start_num..].find(',') {
                if let Ok(num) = body[start_num..start_num + end_num].trim().parse::<u32>() {
                    total_pages = num;
                }
            }
        }

        for page in 1..=total_pages {
            println!("haciendo request para sub página {}", page);

            let url = format!("https://animeav1.com/catalogo?letter={}&page={}",letter, page);
            let body = reqwest::blocking::get(url)?.text()?;

            if let Some(start_idx) = body.find(start_pat) {
                let start = start_idx + 8; 
                if let Some(end_idx) = body[start..].find(end_pat) {
                    let end = start + end_idx;
                    let raw_js = &body[start..end]; 

                    let mut cleaned = raw_js.trim().to_string();

                    cleaned = cleaned
                        .replace(":a", ":{name:\"TV Anime\"}")
                        .replace(":b", ":{name:\"Especial\"}")
                        .replace(":c", ":{name:\"Película\"}")
                        .replace(":d", ":{name:\"OVA\"}")
                        .replace("void 0", "null");

                    if !cleaned.ends_with(']') {
                        cleaned.push(']');
                    }

                    if !cleaned.starts_with('[') {
                        cleaned = format!("[{}", cleaned);
                    }

                    cleaned = cleaned.replace("[[", "[").replace("]]", "]");

                    match json5::from_str::<Vec<Anime>>(&cleaned) {
                        Ok(animes) => {
                            println!("✅ Encontrados {} animes", animes.len());

                            for a in animes {

                                wtr.write_record(&[
                                    a.id, 
                                    a.title, 
                                    a.slug, 
                                    a.synopsis,
                                    a.category.and_then(|c| c.name).unwrap_or_default(),
                                ])?;
                            }
                            println!("✅ Escritos exitosamente en el csv");
                        }
                        Err(e) => {
                            eprintln!("❌ Error persistente: {}", e);
                            let len = cleaned.len();
                            println!("Final del bloque: ...{}", &cleaned[len-20..]);
                        }
                    }
                }
            }
        }
    }
    wtr.flush()?;
    Ok(())
}
