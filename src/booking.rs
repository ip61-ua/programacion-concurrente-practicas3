use std::sync::Arc;
use std::sync::atomic::Ordering;
use crate::venue::{self, ConcurrentSector};

/// Trait que define las operaciones del motor de reservas (*Booker*).
///
/// Modela la interfaz que utiliza la simulación (`simulation.rs`) para consultar
/// disponibilidad, reservar provisionalmente localidades y consolidar la compra.
pub trait Booker {
    /// Nombre del recinto actual.
    #[allow(dead_code)]
    fn get_venue_name(&self) -> &str;

    /// Capacidad total acumulada de todos los sectores del recinto.
    #[allow(dead_code)]
    fn get_total_capacity(&self) -> usize;

    /// Número total de localidades disponibles actualmente en el recinto.
    #[allow(dead_code)]
    fn get_total_available_seats(&self) -> usize;

    /// Lista de códigos de los sectores que aún disponen de plazas libres.
    fn get_available_sectors(&self) -> Vec<String>;

    /// Indica si un sector tiene asientos numerados (`true`) o es de admisión general (`false`).
    #[allow(dead_code)]
    fn is_sector_numbered(&self, sector_code: &str) -> bool;

    /// Lista de identificadores de localidades disponibles en el sector indicado.
    fn get_available_seats(&self, sector_code: &str) -> Vec<String>;

    /// Número de entradas que faltan por vender en todo el recinto.
    fn remaining_seats(&self) -> usize;

    /// Notifica que una sesión de compra va a interactuar con un sector.
    fn select_sector(&mut self, sector_code: &str);

    /// Notifica que una sesión de compra abandona el sector sin completar la compra.
    fn unselect_sector(&mut self, sector_code: &str);

    /// Cancela la reserva provisional de las localidades indicadas,
    /// devolviéndolas al estado `Available`.
    fn deselect_seats(&mut self, sector_code: &str, seat_ids: &[String]) -> Result<(), String>;

    /// Bloquea temporalmente las localidades indicadas cambiando su estado a `Selected`.
    ///
    /// Retorna `Ok(())` si todas se han podido reservar provisionalmente o `Err` si alguna ya no estaba disponible.
    fn select_seats(&mut self, sector_code: &str, seat_ids: &[String]) -> Result<(), String>;

    /// Confirma definitivamente la compra de las localidades indicadas pasando su estado a `Booked`.
    ///
    /// Retorna `Ok(Vec<String>)` con los identificadores únicos definitivos de las entradas vendidas
    /// o `Err` si la reserva provisional no era válida o expiró.
    fn book_seats(&mut self, sector_code: &str, seat_ids: &[String]) -> Result<Vec<String>, String>;

    /// Devuelve una copia/instantánea del recinto (`Venue`) con el estado de todos sus sectores.
    #[allow(dead_code)]
    fn get_venue(&self) -> venue::Venue;
}

// ============================================================
// Implementación secuencial de referencia
// ============================================================

/// Implementación secuencial de referencia del trait `Booker`.
///
/// Modifica directamente la estructura `Venue` en memoria sin sincronización multihilo.
pub struct SequentialBooker {
    venue: venue::Venue,
}

impl SequentialBooker {
    #[allow(dead_code)]
    pub fn new(venue: venue::Venue) -> Self {
        Self { venue }
    }
}

impl Booker for SequentialBooker {
    fn get_venue_name(&self) -> &str {
        &self.venue.name
    }

    fn get_total_capacity(&self) -> usize {
        self.venue.total_capacity()
    }

    fn get_total_available_seats(&self) -> usize {
        self.venue.total_available_seats()
    }

    fn get_available_sectors(&self) -> Vec<String> {
        self.venue
            .sectors
            .iter()
            .filter(|s| s.n_available_seats() > 0)
            .map(|s| s.code.clone())
            .collect()
    }

    fn is_sector_numbered(&self, sector_code: &str) -> bool {
        self.venue
            .sectors
            .iter()
            .find(|s| s.code == sector_code)
            .map(|s| s.is_numbered)
            .unwrap_or(false)
    }

    fn get_available_seats(&self, sector_code: &str) -> Vec<String> {
        if let Some(sector) = self.venue.sectors.iter().find(|s| s.code == sector_code) {
            sector.get_available_seats().iter().map(|s| s.id.clone()).collect()
        } else {
            panic!("Unknown sector: '{sector_code}'");
        }
    }

    fn remaining_seats(&self) -> usize {
        self.venue.total_capacity() - self.venue.total_booked_seats()
    }

    fn select_sector(&mut self, _sector_code: &str) {}

    fn unselect_sector(&mut self, _sector_code: &str) {}

    fn deselect_seats(&mut self, sector_code: &str, seat_ids: &[String]) -> Result<(), String> {
        let sector = self
            .venue
            .sectors
            .iter_mut()
            .find(|s| s.code == sector_code)
            .ok_or_else(|| format!("Unknown sector: '{sector_code}'"))?;

        for seat_id in seat_ids {
            match sector.seats.iter().find(|s| s.id == *seat_id) {
                Some(seat) if seat.state == venue::SeatState::Selected => {}
                Some(_) => return Err(format!("Seat {} is not selected", seat_id)),
                None => return Err(format!("Seat {} does not exist in sector {}", seat_id, sector_code)),
            }
        }
        for seat_id in seat_ids {
            if let Some(seat) = sector.seats.iter_mut().find(|s| s.id == *seat_id) {
                seat.state = venue::SeatState::Available;
            }
        }
        Ok(())
    }

    fn select_seats(&mut self, sector_code: &str, seat_ids: &[String]) -> Result<(), String> {
        let sector = self
            .venue
            .sectors
            .iter_mut()
            .find(|s| s.code == sector_code)
            .ok_or_else(|| format!("Unknown sector: '{sector_code}'"))?;

        for seat_id in seat_ids {
            match sector.seats.iter().find(|s| s.id == *seat_id) {
                Some(seat) if seat.state == venue::SeatState::Available => {}
                Some(_) => return Err(format!("Seat {} is not available", seat_id)),
                None => return Err(format!("Seat {} does not exist in sector {}", seat_id, sector_code)),
            }
        }
        for seat_id in seat_ids {
            if let Some(seat) = sector.seats.iter_mut().find(|s| s.id == *seat_id) {
                seat.state = venue::SeatState::Selected;
            }
        }
        Ok(())
    }

    fn book_seats(&mut self, sector_code: &str, seat_ids: &[String]) -> Result<Vec<String>, String> {
        let sector = self
            .venue
            .sectors
            .iter_mut()
            .find(|s| s.code == sector_code)
            .ok_or_else(|| format!("Unknown sector: '{sector_code}'"))?;

        for seat_id in seat_ids {
            match sector.seats.iter().find(|s| s.id == *seat_id) {
                Some(seat) if seat.state == venue::SeatState::Selected => {}
                Some(seat) if seat.state == venue::SeatState::Booked => {
                    return Err(format!("Seat {} is already booked", seat_id));
                }
                Some(_) => {
                    return Err(format!("Seat {} must be selected before booking", seat_id));
                }
                None => return Err(format!("Seat {} does not exist in sector {}", seat_id, sector_code)),
            }
        }
        for seat_id in seat_ids {
            if let Some(seat) = sector.seats.iter_mut().find(|s| s.id == *seat_id) {
                seat.state = venue::SeatState::Booked;
            }
        }
        Ok(seat_ids.to_vec())
    }

    fn get_venue(&self) -> venue::Venue {
        self.venue.clone()
    }
}

// ============================================================
// Estado compartido del recinto para uso concurrente — sin Mutex
// ============================================================

/// Estado compartido entre todos los hilos compradores.
///
/// Contiene un `ConcurrentSector` por sector — sin ningún `Mutex`.
/// Todas las operaciones de estado de asientos se realizan mediante
/// instrucciones CAS (`compare_exchange`) sobre los `AtomicU8` internos.
///
/// # Garantías
/// - Libre de locks (lock-free) en el hot-path de compra.
/// - `remaining`: `AtomicUsize` decrementado con `AcqRel` al confirmar ventas.
/// - `sector_codes` / `sector_numbered`: inmutables tras construcción, sin sincronización.
pub struct SharedVenueState {
    pub venue_name: String,
    pub total_capacity: usize,
    /// Un `ConcurrentSector` por sector — CAS interno, sin Mutex.
    pub sectors: Vec<ConcurrentSector>,
    /// Mapa de código de sector → índice en `sectors` para lookup O(1).
    pub sector_index: std::collections::HashMap<String, usize>,
    /// Códigos de sector pre-almacenados (lectura sin sincronización).
    sector_codes: Vec<String>,
    /// Flags de numeración por sector (lectura sin sincronización).
    sector_numbered: Vec<bool>,
    /// Contador atómico global de asientos restantes (capacity − booked).
    /// Decrementado con `AcqRel` al confirmar cada venta.
    pub remaining: std::sync::atomic::AtomicUsize,
}

impl SharedVenueState {
    /// Construye el estado compartido a partir del `Venue` secuencial.
    pub fn from_venue(venue: &venue::Venue) -> Self {
        let mut sectors = Vec::with_capacity(venue.sectors.len());
        let mut sector_index = std::collections::HashMap::with_capacity(venue.sectors.len());
        let mut sector_codes = Vec::with_capacity(venue.sectors.len());
        let mut sector_numbered = Vec::with_capacity(venue.sectors.len());

        for (i, s) in venue.sectors.iter().enumerate() {
            sector_index.insert(s.code.clone(), i);
            sector_codes.push(s.code.clone());
            sector_numbered.push(s.is_numbered);
            sectors.push(ConcurrentSector::from_sector(s));
        }

        let total_capacity = venue.total_capacity();

        SharedVenueState {
            venue_name: venue.name.clone(),
            total_capacity,
            sectors,
            sector_index,
            sector_codes,
            sector_numbered,
            remaining: std::sync::atomic::AtomicUsize::new(total_capacity),
        }
    }

    /// Busca el índice de un sector por su código. Panics si no existe.
    #[inline]
    pub fn sector_idx(&self, code: &str) -> usize {
        *self.sector_index.get(code).unwrap_or_else(|| panic!("Unknown sector: '{}'", code))
    }

    /// Reconstruye un `Venue` secuencial haciendo un snapshot atómico del estado.
    pub fn to_venue(&self) -> venue::Venue {
        let sectors = self.sectors.iter().map(|s| s.to_sector()).collect();
        venue::Venue {
            name: self.venue_name.clone(),
            sectors,
        }
    }
}

// ============================================================
// ConcurrentBooker — Booker lock-free sobre SharedVenueState
// ============================================================

/// Implementación concurrente del trait `Booker`.
///
/// Cada hilo comprador tiene su propia instancia de `ConcurrentBooker`,
/// pero todos comparten el mismo `Arc<SharedVenueState>` subyacente.
///
/// **Todas las operaciones de estado de asientos son lock-free**:
/// usan instrucciones CAS (`compare_exchange` sobre `AtomicU8`) en lugar
/// de Mutex. Múltiples hilos pueden operar simultáneamente sobre el mismo
/// sector sin esperar el uno al otro, con rollback automático ante conflictos.
pub struct ConcurrentBooker {
    pub state: Arc<SharedVenueState>,
}

impl ConcurrentBooker {
    pub fn new(state: Arc<SharedVenueState>) -> Self {
        ConcurrentBooker { state }
    }
}

impl Booker for ConcurrentBooker {
    fn get_venue_name(&self) -> &str {
        &self.state.venue_name
    }

    fn get_total_capacity(&self) -> usize {
        self.state.total_capacity
    }

    /// Suma los contadores atómicos aproximados por sector — sin ningún lock.
    fn get_total_available_seats(&self) -> usize {
        self.state
            .sectors
            .iter()
            .map(|s| s.n_available.load(Ordering::Relaxed))
            .sum()
    }

    /// Filtra sectores por su contador atómico de disponibles — cero locks.
    /// Incorpora salvaguarda de vivacidad (liveness) cuando quedan pocas plazas.
    fn get_available_sectors(&self) -> Vec<String> {
        let mut available: Vec<String> = self.state
            .sectors
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                if s.n_available.load(Ordering::Relaxed) > 0 {
                    Some(self.state.sector_codes[i].clone())
                } else {
                    None
                }
            })
            .collect();

        // Salvaguarda de liveness: si ningún sector tiene disponibles inmediatos pero
        // aún quedan entradas por vender en el recinto, ofrecer sectores con aforo no vendido
        // (por si hay reservas provisionales en vuelo pendientes de confirmación o cancelación).
        if available.is_empty() && self.remaining_seats() > 0 {
            for (i, s) in self.state.sectors.iter().enumerate() {
                if s.capacity > s.n_booked.load(Ordering::Relaxed) {
                    available.push(self.state.sector_codes[i].clone());
                }
            }
        }

        available
    }

    /// Lectura sin lock: dato pre-computado e inmutable.
    fn is_sector_numbered(&self, sector_code: &str) -> bool {
        let idx = self.state.sector_idx(sector_code);
        self.state.sector_numbered[idx]
    }

    /// Recorre los `AtomicU8` del sector con cargas `Acquire` — sin lock.
    fn get_available_seats(&self, sector_code: &str) -> Vec<String> {
        let idx = self.state.sector_idx(sector_code);
        self.state.sectors[idx].available_seat_ids()
    }

    /// Lectura atómica global — sin lock.
    fn remaining_seats(&self) -> usize {
        self.state.remaining.load(Ordering::Relaxed)
    }

    /// No-op en la implementación concurrente.
    fn select_sector(&mut self, _sector_code: &str) {}

    /// No-op en la implementación concurrente.
    fn unselect_sector(&mut self, _sector_code: &str) {}

    /// `Selected → Available` mediante CAS atómico — sin lock.
    fn deselect_seats(&mut self, sector_code: &str, seat_ids: &[String]) -> Result<(), String> {
        let idx = self.state.sector_idx(sector_code);
        self.state.sectors[idx].deselect_seats(seat_ids)
    }

    /// `Available → Selected` mediante `compare_exchange(AcqRel)` — lock-free con rollback.
    fn select_seats(&mut self, sector_code: &str, seat_ids: &[String]) -> Result<(), String> {
        let idx = self.state.sector_idx(sector_code);
        self.state.sectors[idx].select_seats(seat_ids)
    }

    /// `Selected → Booked` mediante `compare_exchange(AcqRel)` — lock-free con rollback.
    ///
    /// Decrementa el contador global `remaining` con `AcqRel` tras confirmar la venta.
    fn book_seats(&mut self, sector_code: &str, seat_ids: &[String]) -> Result<Vec<String>, String> {
        let idx = self.state.sector_idx(sector_code);
        let result = self.state.sectors[idx].book_seats(seat_ids);
        if let Ok(ref tickets) = result {
            self.state
                .remaining
                .fetch_sub(tickets.len(), Ordering::AcqRel);
        }
        result
    }

    /// Snapshot del recinto mediante cargas atómicas `Acquire` — sin locks.
    fn get_venue(&self) -> venue::Venue {
        self.state.to_venue()
    }
}
