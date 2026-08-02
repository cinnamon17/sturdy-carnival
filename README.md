# 🎌 Anime Indexer & Stremio Addon Engine ETL

<p align="center">
  <img src="https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white" alt="Rust" />
  <img src="https://img.shields.io/badge/Axum-000000?style=for-the-badge" alt="Axum" />
  <img src="https://img.shields.io/badge/MySQL-4479A1?style=for-the-badge&logo=mysql&logoColor=white" alt="MySQL" />
  <img src="https://img.shields.io/badge/Tokio-Async-FF4500?style=for-the-badge" alt="Tokio" />
  <img src="https://img.shields.io/badge/Stremio-Addon--Ready-8A2BE2?style=for-the-badge" alt="Stremio" />
</p>

Pipeline de extracción, transformación y carga (ETL) y servidor HTTP Addon desarrollado en **Rust**. Extrae catálogos de anime, enriquece sus identificadores globales (`imdb_id`, `kitsu_id`), vincula sus torrents desde Nyaa.si y sirve streams directamente a **Stremio** integrándose con la API v4 de **AllDebrid**.

---

## 🛠️ Arquitectura del Sistema

El proyecto consta de 4 etapas de procesamiento ETL más un motor de servidor web en tiempo real:

```text
[ MyAnimeList API ] ──(1. Scraping)──> [ animes_dump.jsonl ]
                                                │
                                                v
[ Fribb Database ]  ───(2. Enrich)────> [ animes_enriched.jsonl ]
                                                │
                                                v
                                         (3. Sync MySQL)
                                                │
                                                v
[ Nyaa.si RSS Engine ] ─> (4. Torrent Match)──> [ MySQL DB ]
                                                    │
                                                    v
[ Stremio App ] <──(HTTPS / Cloudflare)── [ Axum HTTP Server ] ──> [ AllDebrid API v4 ]

```

### 🔀 Etapas del Proceso:

1. **Scraper MAL (`src/scraper.rs`)**: Descarga el ranking e información de animes desde MyAnimeList API v2 con manejo de rate-limiting (HTTP 429) y guardado en streaming a `.jsonl`.
2. **Enriquecimiento (`src/enrich.rs`)**: Cruza el dump con el mapping de *Fribb* para inyectar IDs equivalentes (`imdb_id`, `kitsu_id`, `anilist_id`).
3. **Persistencia SQL (`src/db.rs`)**: Importación por lotes transaccionales a **MySQL** usando `sqlx`.
4. **Scraper Nyaa RSS (`src/nyaa.rs`)**: Consulta el índice de torrents de Nyaa.si, extrae metadatos (episodios, resolución, grupo de release, seeders) y genera los magnet links.
5. **Servidor HTTP Axum (`src/server.rs`)**: Sirve el `manifest.json` y los streams para Stremio. Despacha la resolución del magnet vía AllDebrid únicamente cuando el usuario presiona *Play*.

---

## 🛠️ Requisitos Previos

* **Rust** (edición 2021 o superior)
* **MySQL / MariaDB**
* Credenciales Client ID de **MyAnimeList**
* Cuenta y API Key de **AllDebrid** (para reproducir los streams en Stremio)

---

## ⚙️ Configuración

1. Clona el repositorio:
   ```bash
   git clone [https://github.com/tu-usuario/anime-stremio-pipeline.git](https://github.com/tu-usuario/anime-stremio-pipeline.git)
   cd anime-stremio-pipeline

2. Crea un archivo `.env` en la raíz del proyecto basándote en la siguiente plantilla:

   ```env
   DATABASE_URL="mysql://usuario:password@localhost:3306/anime_db"
   MAL_CLIENT_ID="tu_mal_client_id_aqui"
   BASE_URL=https://stremio.lify.win

   ```
---

## 🏃 Compilación y Ejecución

### 📦 Compilar Versión Release (Producción)

```bash
cargo build --release

```

El ejecutable optimizado se generará en `./target/release/sturdy-carnival`.

---

### 💻 Subcomandos Disponibles

Puedes ejecutar subcomandos con `cargo run -- <comando>` o mediante el binario `./target/release/stremio_anime_indexer <comando>`:

| Comando | Descripción |
| --- | --- |
| **`all`** | Ejecuta todo el pipeline ETL secuencialmente (Fases 1 a 4). |
| **`mal`** | **Paso 1:** Descarga el ranking y metadatos desde MyAnimeList (`animes_dump.jsonl`). |
| **`enrich`** | **Paso 2:** Inyecta equivalencias de IMDb, Kitsu y AniList desde Fribb (`animes_enriched.jsonl`). |
| **`db-sync`** | **Paso 3:** Sincroniza e inserta el archivo JSONL enriquecido dentro de MySQL. |
| **`nyaa`** | **Paso 4:** Busca e indexa los torrents y magnet links desde Nyaa.si hacia MySQL. |
| **`serve`** | **Servidor:** Arranca el servidor HTTP Axum para Stremio en el puerto configurado (`7000`). |

#### Ejemplos de uso:

```bash
# Iniciar el servidor web de Stremio
cargo run -- serve

# Ejecutar el scraper de Nyaa en segundo plano con el binario release
./target/release/sturdy-carnival nyaa

```

---


## 📱 Instalación del Addon en Stremio

1. Obtén tu **API Key** desde el panel de [AllDebrid API](https://alldebrid.com/apikeys/).
2. Añade el manifiesto directamente en Stremio pegando esta URL:
  ```text
  https://stremio.lify.win/TU_ALLDEBRID_API_KEY/manifest.json
  ```

---

## 🌙 Ejecución Desatendida en Servidor (Producción)

### 1. Servidor y Scraper con `tmux`

Puedes mantener el servidor HTTP y la re-indexación corriendo en sesiones independientes de `tmux`:

```bash
# Sesión para el Servidor HTTP
tmux new -s stremio-server
./target/release/sturdy-carnival serve
# Desconectar: Ctrl + B y luego D

# Sesión para el Scraper de Nyaa
tmux new -s nyaa-scraper
./target/release/sturdy-carnival nyaa
# Desconectar: Ctrl + B y luego D

```

---

### 2. Cron Job Nocturno

Programa el escaneo desatendido del pipeline agregando esta línea en `crontab -e`:

```cron
# Ejecutar el pipeline ETL completo cada noche a las 03:00 AM
0 3 * * * cd /ruta/absoluta/a/proyecto && ./target/release/sturdy-carnival all >> ./pipeline.log 2>&1

```

---

## 📊 Esquema de la Base de Datos

* **`animes`**: Almacena `mal_id`, metadatos, fecha de escaneo (`last_scraped_at`) y los identificadores de mapeo (`kitsu_id`, `imdb_id`, `anilist_id`).
* **`torrents`**: Relaciona `anime_id` con `info_hash` (Primary/Unique Key), `title`, `magnet`, `episode`, `resolution`, `release_group`, `seeders` y `leechers`.
