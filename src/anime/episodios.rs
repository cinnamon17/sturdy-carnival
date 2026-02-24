use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use javascript::evaluate_script;

#[derive(Debug, Deserialize, Clone)]
pub struct Genre { pub name: String }

#[derive(Debug, Deserialize, Clone)]
pub struct Category { pub name: String }

#[derive(Debug, Deserialize, Clone)]
pub struct LinkItem { pub server: String, pub url: String }

#[derive(Debug, Deserialize, Clone)]
pub struct EpisodeDetail {
    pub number: u32,
    pub title: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MediaDetail {
    pub title: String,
    pub genres: Vec<Genre>,
    pub synopsis: String,
    pub episodes_count: u32,
    pub score: f32,
    pub slug: String,
}

#[derive(Debug, Deserialize)]
pub struct SvelteWrapper {
    pub episode: Option<EpisodeDetail>,
    pub embeds: Option<HashMap<String, Vec<LinkItem>>>,
    pub downloads: Option<HashMap<String, Vec<LinkItem>>>,
}

#[derive(Debug, Deserialize)]
struct Record { pub slug: String }

// Esta es la estructura que irá al CSV final
#[derive(Debug, Serialize)]
struct CsvOutput {
    titulo: String,
    episodio: u32,
    generos: String,
    score: f32,
    sinopsis: String,
    pixel_drain_embed: String,
    mega_embed: String,
    pixel_drain_download: String,
    mega_download: String,
}


pub fn ejecutar_scraper() {

}

fn extraer_json(data: &String) -> String{

    let result = evaluate_script(format!("JSON.parse({})", data), None::<&std::path::Path>).unwrap();
    match result {
        javascript::Value::String(str) => String::from_utf16(&str).expect("Error al decodificar utf16"),
        _ => panic!("Se esperaba un string pero se recibió otro tipo de javascript")
    }
}

fn extraer_media_info(body: &str) -> Result<MediaDetail, Box<dyn Error>> {
    let start_idx = body.find("media:{").ok_or("No se encontró 'media:'")? + 6;
    let mut depth = 0;
    let mut end_idx = 0;

    // Recorremos el string desde la primera llave para encontrar su pareja exacta
    for (i, c) in body[start_idx..].char_indices() {
        if c == '{' { depth += 1; }
        else if c == '}' {
            depth -= 1;
            if depth == 0 {
                end_idx = start_idx + i + 1;
                break;
            }
        }
    }

    if end_idx == 0 { return Err("No se pudo balancear las llaves de media".into()); }

    let json_str = body[start_idx..end_idx].replace("void 0", "null");

    let media: MediaDetail = json5::from_str(&json_str)
        .map_err(|e| format!("Error en Media: {}\nJSON: {}", e, json_str))?;
    Ok(media)
}

fn extraer_episodio_info(body: &str, media: &MediaDetail) -> Result<CsvOutput, Box<dyn Error>> {
    // Aplicamos la misma lógica para el bloque de episodio
    let start_idx = body.find("episode:{").ok_or("No se encontró 'episode:'")? + 8;
    let mut depth = 0;
    let mut end_idx = 0;

    for (i, c) in body[start_idx..].char_indices() {
        if c == '{' { depth += 1; }
        else if c == '}' {
            depth -= 1;
            if depth == 0 {
                end_idx = start_idx + i + 1;
                break;
            }
        }
    }

    if end_idx == 0 { return Err("No se pudo balancear las llaves de episodio".into()); }

    let raw_content = &body[start_idx..end_idx];

    // El bloque del episodio en Svelte a veces NO es un objeto único, sino una lista de campos.
    // Si el parseo falla, envolvemos en { ... }
    let json_str = raw_content.replace("void 0", "null");

    // Intentamos parsear como SvelteWrapper (que espera los campos episode, embeds, etc)
    // Si falla, es que necesitamos el wrapper manual
    let wrapper: SvelteWrapper = json5::from_str(&json_str)
        .or_else(|_| json5::from_str(&format!("{{ {} }}", json_str)))
        .map_err(|e| format!("Error en Episodio: {}\nJSON: {}", e, json_str))?;

    let ep = wrapper.episode.ok_or("No hay datos de episodio")?;

    let get_link = |map: &Option<HashMap<String, Vec<LinkItem>>>, server_name: &str| -> String {
        map.as_ref()
            .and_then(|m| m.values().flatten().find(|l| l.server.to_lowercase().contains(server_name)))
            .map(|l| l.url.clone())
            .unwrap_or_default()
    };

    Ok(CsvOutput {
        titulo: media.title.clone(),
        episodio: ep.number,
        generos: media.genres.iter().map(|g| g.name.clone()).collect::<Vec<_>>().join(", "),
        score: media.score,
        sinopsis: media.synopsis.clone(),
        pixel_drain_embed: get_link(&wrapper.embeds, "pdrain"),
        mega_embed: get_link(&wrapper.embeds, "mega"),
        pixel_drain_download: get_link(&wrapper.downloads, "pdrain"),
        mega_download: get_link(&wrapper.downloads, "mega"),
    })
}

fn parse_html(html: &String) -> &str{

    let start_data = match html.find("data: [null,"){

        Some(index) =>{
            index
        },

        None => panic!("fallo en la memoría")
    };
    let end_data = match html.find("parent:1}}]"){

        Some(index) => {
            index + 11
        },
        None => panic!("fallo en la memoría abortando")
    };

    let data = &html[start_data..end_data];

    data.trim()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_json_parser() {

        let html = r#"<script>
                                {
                                        __sveltekit_xxkd4j = {
                                                base: new URL("../..", location).pathname.slice(0, -1),
                                                env: {"PUBLIC_DISCORD_CLIENT_ID":"1331447534710947961","PUBLIC_GOOGLE_ANALYTICS_ID":"G-T7KQ8V9457","PUBLIC_GOOGLE_CLIENT_ID":"891601101242-5obguq563bf0uf973gm626s0kr7ghsu9.apps.googleusercontent.com","PUBLIC_NAME":"AnimeAV1","PUBLIC_TURNSTILE_KEY":"0x4AAAAAAA57OaBDuXrlIQhk","PUBLIC_CDN_URL":"https://cdn.animeav1.com","PUBLIC_URL":"https://animeav1.com"}
                                        };

                                        const element = document.currentScript.parentElement;

                                        Promise.all([
                                                import("../../_app/immutable/entry/start.CONYJzDq.js"),
                                                import("../../_app/immutable/entry/app.BYlhWcTT.js")
                                        ]).then(([kit, app]) => {
                                                kit.start(app, element, {
                                                        node_ids: [0, 2, 4, 14],
                                                        data: [null,{type:"data",data:{user:null},uses:{dependencies:["https://animeav1.com/media/ao-no-miburo-serizawa-ansatsu-hen/auth"]}},{type:"data",data:{media:{id:3409,categoryId:1,title:"Ao no Miburo: Serizawa Ansatsu-hen",aka:{"en-us":"Blue Miburo Season 2","ja-jp":"青のミブロ 芹沢暗殺編"},genres:[{id:1,name:"Acción",type:0,slug:"accion",malId:1},{id:13,name:"Shounen",type:0,slug:"shounen",malId:27},{id:28,name:"Histórico",type:1,slug:"historico",malId:13},{id:42,name:"Samurai",type:1,slug:"samurai",malId:21}],synopsis:"Segunda temporada de Ao no Miburo.",poster:null,backdrop:null,trailer:null,status:2,runtime:null,startDate:"2025-12-20",nextDate:null,endDate:null,waitDays:7,featured:false,mature:false,episodesCount:9,score:0,votes:0,slug:"ao-no-miburo-serizawa-ansatsu-hen",malId:61333,seasons:null,createdAt:"2025-12-20 16:46:57.983135+00",updatedAt:"2025-12-20 16:48:14.039+00",category:{id:1,name:"TV Anime",slug:"tv-anime",malId:1},episodes:[{id:48454,number:1},{id:48725,number:2},{id:49314,number:3},{id:49808,number:4},{id:50384,number:5},{id:50827,number:6},{id:51239,number:7},{id:51374,number:8},{id:51839,number:9}],relations:[{type:1,destination:{id:285,slug:"ao-no-miburo",title:"Ao no Miburo",startDate:"2024-10-19"}}]}},uses:{params:["slug"]}},{type:"data",data:{episode:{id:48454,mediaId:3409,title:null,number:1,season:null,relativeNumber:null,variants:{SUB:1},filler:false,publishedAt:"2025-12-20 17:00:43.893+00",createdAt:"2025-12-20 16:59:39.476496+00",updatedAt:"2025-12-20 17:00:43.894262+00",mirrors:void 0},embeds:{SUB:[{server:"HLS",url:"https://player.zilla-networks.com/play/d3f27eaa1020637ee05e1345c02b2153"},{server:"PDrain",url:"https://pixeldrain.com/u/Eme4828r?embed"},{server:"UPNShare",url:"https://animeav1.uns.bio/#9rmpfx"},{server:"MP4Upload",url:"https://www.mp4upload.com/embed-fxfuctj5m6b7.html"}]},downloads:{SUB:[{server:"PDrain",url:"https://pixeldrain.com/u/Eme4828r"},{server:"MP4Upload",url:"https://www.mp4upload.com/fxfuctj5m6b7"},{server:"1Fichier",url:"https://1fichier.com/?3crgenl7248xghq73gte"}]}},uses:{params:["number"],parent:1}}],
                                                        form: null,
                                                        error: null
                                                });
                                        });
                                }
                        </script>"#;

                        let parsed_data = r#"data: [null,{type:"data",data:{user:null},uses:{dependencies:["https://animeav1.com/media/ao-no-miburo-serizawa-ansatsu-hen/auth"]}},{type:"data",data:{media:{id:3409,categoryId:1,title:"Ao no Miburo: Serizawa Ansatsu-hen",aka:{"en-us":"Blue Miburo Season 2","ja-jp":"青のミブロ 芹沢暗殺編"},genres:[{id:1,name:"Acción",type:0,slug:"accion",malId:1},{id:13,name:"Shounen",type:0,slug:"shounen",malId:27},{id:28,name:"Histórico",type:1,slug:"historico",malId:13},{id:42,name:"Samurai",type:1,slug:"samurai",malId:21}],synopsis:"Segunda temporada de Ao no Miburo.",poster:null,backdrop:null,trailer:null,status:2,runtime:null,startDate:"2025-12-20",nextDate:null,endDate:null,waitDays:7,featured:false,mature:false,episodesCount:9,score:0,votes:0,slug:"ao-no-miburo-serizawa-ansatsu-hen",malId:61333,seasons:null,createdAt:"2025-12-20 16:46:57.983135+00",updatedAt:"2025-12-20 16:48:14.039+00",category:{id:1,name:"TV Anime",slug:"tv-anime",malId:1},episodes:[{id:48454,number:1},{id:48725,number:2},{id:49314,number:3},{id:49808,number:4},{id:50384,number:5},{id:50827,number:6},{id:51239,number:7},{id:51374,number:8},{id:51839,number:9}],relations:[{type:1,destination:{id:285,slug:"ao-no-miburo",title:"Ao no Miburo",startDate:"2024-10-19"}}]}},uses:{params:["slug"]}},{type:"data",data:{episode:{id:48454,mediaId:3409,title:null,number:1,season:null,relativeNumber:null,variants:{SUB:1},filler:false,publishedAt:"2025-12-20 17:00:43.893+00",createdAt:"2025-12-20 16:59:39.476496+00",updatedAt:"2025-12-20 17:00:43.894262+00",mirrors:void 0},embeds:{SUB:[{server:"HLS",url:"https://player.zilla-networks.com/play/d3f27eaa1020637ee05e1345c02b2153"},{server:"PDrain",url:"https://pixeldrain.com/u/Eme4828r?embed"},{server:"UPNShare",url:"https://animeav1.uns.bio/#9rmpfx"},{server:"MP4Upload",url:"https://www.mp4upload.com/embed-fxfuctj5m6b7.html"}]},downloads:{SUB:[{server:"PDrain",url:"https://pixeldrain.com/u/Eme4828r"},{server:"MP4Upload",url:"https://www.mp4upload.com/fxfuctj5m6b7"},{server:"1Fichier",url:"https://1fichier.com/?3crgenl7248xghq73gte"}]}},uses:{params:["number"],parent:1}}]"#;

                        let body = &html.to_string();

                        let result = parse_html(body);

                        assert_eq!(result, parsed_data);

    }
}
