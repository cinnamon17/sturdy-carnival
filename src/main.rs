use std::error::Error;

use sturdy_carnival::anime::catalogo::extraer_catalogo;
use sturdy_carnival::anime::episodios::ejecutar_scraper;

fn main() -> Result<(), Box<dyn Error>> {
    ejecutar_scraper()

}
