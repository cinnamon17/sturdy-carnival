# 🎌 Anime Indexer & Stremio Pipeline ETL

<p align="center">
  <img src="https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white" alt="Rust" />
  <img src="https://img.shields.io/badge/MySQL-4479A1?style=for-the-badge&logo=mysql&logoColor=white" alt="MySQL" />
  <img src="https://img.shields.io/badge/Tokio-Async-FF4500?style=for-the-badge" alt="Tokio" />
  <img src="https://img.shields.io/badge/Stremio-Addon--Ready-8A2BE2?style=for-the-badge" alt="Stremio" />
</p>

Pipeline de extracción, transformación y carga (ETL) desarrollado en **Rust** de alto rendimiento. Extrae catálogos de anime, enriquece sus identificadores globales y vincula automáticamente sus magnet links y torrents desde Nyaa.si para alimentar un motor de Addon de **Stremio + AllDebrid**.

---

## 🛠️ Arquitectura del Pipeline

El proyecto procesa y relaciona datos en 4 etapas secuenciales automatizables:

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
[ Nyaa.si RSS Engine ] ─> (4. Torrent Match)──> [ MySQL DB ] ──> [ Stremio Addon Engine ]

```

### 🔀 Etapas del Proceso:

1. **Scraper MAL (`src/scraper.rs`)**:
Descarga el ranking e información detallada de animes desde MyAnimeList API v2 usando manejo de rate-limiting (HTTP 429) y guardado streaming a `.jsonl`.
2. **Enriquecimiento (`src/enrich.rs`)**:
Cruza el dump con el repositorio de mapping de *Fribb* para inyectar IDs externos equivalentes (`imdb_id`, `kitsu_id`, `anilist_id`).
3. **Persistencia SQL (`src/db.rs`)**:
Importación por lotes transaccionales (`batch commits`) normalizando fechas y metadatos estructurados en **MySQL** mediante `sqlx`.
4. **Scraper Nyaa RSS (`src/nyaa.rs`)**:
Consulta el índice de torrents de Nyaa.si por cada anime en la BD (`c=0_0` para máxima cobertura de idiomas y subtítulos), extrayendo metadatos como episodios, grupos de release (ej: *SubsPlease*, *Erai-raws*), resoluciones (`1080p`, `720p`), `seeders`, `leechers` y generando los enlaces `magnet` listos para resolver.

---

## 🛠️ Requisitos Previos

* **Rust** (edición 2021 o superior)
* **MySQL / MariaDB**
* Credenciales de API Client ID de **MyAnimeList**

---

## ⚙️ Configuración

1. Clona el repositorio:
```bash
git clone [https://github.com/tu-usuario/anime-stremio-pipeline.git](https://github.com/tu-usuario/anime-stremio-pipeline.git)
cd anime-stremio-pipeline

2. Crea un archivo `.env` en la raíz del proyecto basándote en el siguiente formato:



```env
DATABASE_URL="mysql://usuario:password@localhost:3306/anime_db"
MAL_CLIENT_ID="tu_mal_client_id_aqui"

```



---

## 🏃 Compilación y Ejecución

El binario incluye una CLI interactiva basada en subcomandos para ejecutar pasos específicos o todo el pipeline completo.

### 📦 Compilar Versión Release (Producción)

Para obtener la máxima eficiencia en rendimiento durante la extracción masiva:

```bash
cargo build --release

```

El ejecutable optimizado se generará en `./target/release/stremio_anime_indexer`.

---

### 💻 Subcomandos Disponibles

Puedes lanzar cualquier fase de forma independiente utilizando `cargo run -- <comando>` o directamente mediante el binario compilado `./target/release/stremio_anime_indexer <comando>`:

| Comando | Descripción |
| --- | --- |
| **`all`** | Ejecuta el pipeline completo de principio a fin (Fases 1 a 4). |
| **`mal`** | **Paso 1:** Descarga el ranking global y metadatos desde MyAnimeList (`animes_dump.jsonl`). |
| **`enrich`** | **Paso 2:** Inyecta las equivalencias de IMDb, Kitsu y AniList desde Fribb (`animes_enriched.jsonl`). |
| **`db-sync`** | **Paso 3:** Sincroniza e inserta el archivo JSONL enriquecido dentro de MySQL. |
| **`nyaa`** | **Paso 4:** Busca e indexa todos los torrents y magnet links desde Nyaa.si hacia MySQL. |

#### Ejemplos de uso rápido:

```bash
# Ejecutar todo el proceso secuencial en modo desarrollo
cargo run -- all

# Ejecutar únicamente la indexación de torrents desde Nyaa con el binario release
./target/release/stremio_anime_indexer nyaa

```

---

## 🌙 Ejecución Desatendida en Servidor (Producción)

### 1. Ejecución nocturna manual con `tmux`

Para dejar corriendo la indexación en segundo plano sin riesgo a interrupciones si se cierra la conexión SSH:

```bash
# 1. Crear nueva sesión en tmux
tmux new -s nyaa-scraper

# 2. Iniciar el subcomando deseado
./target/release/stremio_anime_indexer nyaa

# 3. Desconectarte (Detach): Presiona `Ctrl + B` y luego `D`

```

Para recuperar la sesión más tarde y comprobar los avances:

```bash
tmux attach -t nyaa-scraper

```

---

### 2. Automatización Nocturna con Cron Job

Para programar la sincronización desatendida del pipeline completo en tu servidor Debian/Linux, edita la tabla de cron (`crontab -e`) y añade la siguiente regla:

```cron
# Ejecutar el pipeline completo todas las noches a las 03:00 AM
0 3 * * * cd /ruta/absoluta/a/anime-stremio-pipeline && ./target/release/stremio_anime_indexer all >> ./pipeline.log 2>&1

```

---

## 📊 Esquema de la Base de Datos

El sistema gestiona automáticamente la relación entre el catálogo y los torrents asociados por episodio:

* **`animes`**: Almacena el ID primario (`mal_id`), títulos alternativos, puntuación, imágenes, estado de escaneo (`last_scraped_at`) y los mappings para Stremio (`kitsu_id`, `imdb_id`, `anilist_id`).
* **`torrents`**: Relaciona `anime_id` con `info_hash` (Primary/Unique Key), `title`, `magnet`, `episode`, `resolution`, `release_group`, `seeders` y `leechers`.

---

## 🔮 Próximos Pasos

* [ ] Implementar Servidor HTTP Axum para servir el Manifest de Stremio.
* [ ] Integrar Endpoint de resolvedor con la API v4 de AllDebrid.
* [ ] Optimizar búsqueda difusa de títulos (Fuzzy Matching) para releases no convencionales en Nyaa.
