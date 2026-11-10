/// Parámetros de configuración de la simulación y del recinto de ejemplo.

/// Número mínimo de entradas que un comprador intenta adquirir en una sesión.
pub static MIN_TICKETS: usize = 1;

/// Número máximo de entradas que un comprador intenta adquirir en una sesión.
pub static MAX_TICKETS: usize = 6;

/// Factor multiplicador temporal para acelerar los retardos de simulación en CPU.
pub static TIME_MULTIPLIER: f32 = 100000.0;

/// Número de sectores numerados en el recinto de ejemplo.
pub static N_SECTORS: usize = 25;

/// Capacidad de cada sector numerado en el recinto de ejemplo.
pub static SEATED_SECTOR_CAPACITY: usize = 300;

/// Capacidad del sector Golden Circle en el recinto de ejemplo.
pub static GOLDEN_CIRCLE_CAPACITY: usize = 2500;

/// Capacidad del sector Pista General (Standing) en el recinto de ejemplo.
pub static STANDING_CAPACITY: usize = 12500;
