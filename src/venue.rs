use std::collections::HashMap;
use std::fmt;

// ============================================================
// Modelos originales (requeridos por main.rs y SequentialBooker)
// ============================================================

/// Modelo de un recinto (*Venue*), compuesto por un nombre y múltiples sectores.
#[derive(Clone)]
pub struct Venue {
    pub name: String,
    pub sectors: Vec<Sector>,
}

impl Venue {
    /// Capacidad total sumando todos los sectores.
    pub fn total_capacity(&self) -> usize {
        self.sectors.iter().map(|s| s.capacity).sum()
    }

    /// Total de asientos disponibles sumando todos los sectores.
    pub fn total_available_seats(&self) -> usize {
        self.sectors.iter().map(|s| s.n_available_seats()).sum()
    }

    /// Total de asientos ya vendidos (*Booked*) sumando todos los sectores.
    pub fn total_booked_seats(&self) -> usize {
        self.sectors.iter().map(|s| s.n_booked_seats()).sum()
    }
}

impl fmt::Display for Venue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {}/{}\n",
            self.name,
            self.total_booked_seats(),
            self.total_capacity(),
        )?;

        write!(f, "Sectors:\n")?;

        for sector in &self.sectors {
            write!(
                f,
                "  - {} ({}) {}/{}\n",
                sector.name,
                sector.code,
                sector.n_booked_seats(),
                sector.capacity,
            )?;
        }
        Ok(())
    }
}

/// Representa un sector dentro del recinto.
#[derive(Clone)]
pub struct Sector {
    pub name: String,
    pub code: String,
    pub seats: Vec<Seat>,
    pub capacity: usize,
    pub is_numbered: bool,
}

/// Estado en el que se encuentra un asiento o localidad.
#[derive(PartialEq, Clone)]
pub enum SeatState {
    /// Asiento libre, disponible para selección.
    Available,
    /// Asiento bloqueado provisionalmente por una sesión de compra.
    Selected,
    /// Asiento vendido definitivamente.
    Booked,
}

/// Representa una localidad individual con su estado e identificador único.
#[derive(Clone)]
pub struct Seat {
    pub state: SeatState,
    pub id: String,
}

impl Sector {
    /// Crea un nuevo sector inicializando todos sus asientos en estado `Available`.
    pub fn new(name: &str, code: &str, capacity: usize, is_numbered: bool) -> Sector {
        let seats = (1..=capacity)
            .map(|i| Seat {
                state: SeatState::Available,
                id: format!("{}-{}", code, i),
            })
            .collect();

        Sector {
            name: name.to_string(),
            code: code.to_string(),
            seats,
            capacity,
            is_numbered,
        }
    }

    /// Devuelve referencias a todos los asientos que se encuentran actualmente disponibles.
    pub fn get_available_seats(&self) -> Vec<&Seat> {
        self.seats.iter().filter(|s| s.state == SeatState::Available).collect()
    }

    /// Devuelve el número de asientos actualmente en estado `Available`.
    pub fn n_available_seats(&self) -> usize {
        self.seats.iter().filter(|s| s.state == SeatState::Available).count()
    }

    /// Devuelve el número de asientos actualmente en estado `Booked`.
    pub fn n_booked_seats(&self) -> usize {
        self.seats.iter().filter(|s| s.state == SeatState::Booked).count()
    }
}

// ============================================================
// Estado interno concurrente por sector — full O(1) operations
// ============================================================

/// Estado mutable protegido por `Mutex` para un sector concreto.
///
/// Tres índices garantizan operaciones O(1) amortizadas:
/// - `seat_id_to_idx`: HashMap para resolver seat_id → índice en O(1).
/// - `available`: Vec desordenado de índices disponibles (stack: push/pop O(1)).
/// - `available_pos`: Mapa inverso seat_index → posición en `available` para
///   eliminación O(1) por swap-pop sin búsqueda lineal.
pub struct SectorState {
    /// Nombre descriptivo del sector.
    pub name: String,
    /// Código corto del sector (ej. "ST", "GC", "S1").
    pub code: String,
    /// ¿Los asientos son numerados?
    pub is_numbered: bool,
    /// IDs de todos los asientos del sector (inmutable tras construcción).
    pub seat_ids: Vec<String>,
    /// Mapa de seat_id → índice en `seat_ids` / `seat_states` para lookup O(1).
    seat_id_to_idx: HashMap<String, usize>,
    /// Estado de cada asiento, indexado por posición en `seat_ids`.
    pub seat_states: Vec<SeatState>,
    /// Índices (en `seat_ids`) de los asientos actualmente `Available`.
    /// Se mantiene como pila desordenada: pop/push O(1).
    pub available: Vec<usize>,
    /// Mapa inverso: seat_index → posición en `available`.
    /// Permite eliminar de `available` en O(1) por swap-pop sin búsqueda lineal.
    /// Valor `usize::MAX` indica que el asiento no está en `available`.
    available_pos: Vec<usize>,
    /// Número de asientos en estado `Booked` en este sector.
    pub n_booked: usize,
    /// Capacidad total del sector.
    pub capacity: usize,
}

impl SectorState {
    /// Construye el estado concurrente de un sector a partir del modelo secuencial.
    pub fn from_sector(sector: &Sector) -> Self {
        let n = sector.seats.len();
        let seat_ids: Vec<String> = sector.seats.iter().map(|s| s.id.clone()).collect();
        let seat_states: Vec<SeatState> = sector.seats.iter().map(|s| s.state.clone()).collect();
        // Inicialmente todos los asientos están Available
        let available: Vec<usize> = (0..n).collect();
        // available_pos[i] = i (posición inicial coincide con el índice)
        let available_pos: Vec<usize> = (0..n).collect();

        // Construir HashMap para lookup O(1)
        let seat_id_to_idx: HashMap<String, usize> = seat_ids
            .iter()
            .enumerate()
            .map(|(i, id)| (id.clone(), i))
            .collect();

        SectorState {
            name: sector.name.clone(),
            code: sector.code.clone(),
            is_numbered: sector.is_numbered,
            capacity: sector.capacity,
            seat_ids,
            seat_id_to_idx,
            seat_states,
            available,
            available_pos,
            n_booked: 0,
        }
    }

    /// Número de asientos disponibles actualmente.
    #[inline]
    pub fn n_available(&self) -> usize {
        self.available.len()
    }

    /// Número de asientos vendidos definitivamente.
    #[inline]
    #[allow(dead_code)]
    pub fn n_booked(&self) -> usize {
        self.n_booked
    }

    /// Devuelve los IDs de todos los asientos actualmente disponibles.
    pub fn available_seat_ids(&self) -> Vec<String> {
        self.available.iter().map(|&i| self.seat_ids[i].clone()).collect()
    }

    /// Busca el índice interno de un seat_id. O(1) gracias al HashMap.
    #[inline]
    fn lookup(&self, seat_id: &str) -> Result<usize, String> {
        self.seat_id_to_idx
            .get(seat_id)
            .copied()
            .ok_or_else(|| format!("Seat {} not found in sector {}", seat_id, self.code))
    }

    /// Elimina un seat_index del vector `available` en O(1) usando swap-pop
    /// y actualiza el mapa inverso `available_pos`.
    #[inline]
    fn remove_from_available(&mut self, seat_idx: usize) {
        let pos = self.available_pos[seat_idx];
        debug_assert!(pos < self.available.len());

        let last = self.available.len() - 1;
        if pos != last {
            // Swap con el último elemento
            let swapped_seat_idx = self.available[last];
            self.available.swap(pos, last);
            // Actualizar la posición del elemento que se movió
            self.available_pos[swapped_seat_idx] = pos;
        }
        self.available.pop();
        // Marcar como no-disponible
        self.available_pos[seat_idx] = usize::MAX;
    }

    /// Añade un seat_index al vector `available` y actualiza el mapa inverso.
    #[inline]
    fn add_to_available(&mut self, seat_idx: usize) {
        let pos = self.available.len();
        self.available.push(seat_idx);
        self.available_pos[seat_idx] = pos;
    }

    /// Intenta marcar los asientos indicados como `Selected` (reserva provisional).
    ///
    /// Si algún asiento no está `Available`, retorna `Err` sin modificar nada.
    /// Complejidad: O(k) donde k = número de asientos solicitados (todas las
    /// operaciones internas son O(1) amortizadas).
    pub fn select_seats(&mut self, seat_ids: &[String]) -> Result<(), String> {
        // Encontrar los índices de los asientos solicitados y validarlos
        let mut indices_to_select: Vec<usize> = Vec::with_capacity(seat_ids.len());
        for seat_id in seat_ids {
            let idx = self.lookup(seat_id)?;
            if self.seat_states[idx] != SeatState::Available {
                return Err(format!("Seat {} is not available", seat_id));
            }
            indices_to_select.push(idx);
        }

        // Transición Available → Selected y actualizar el índice de disponibles
        for &idx in &indices_to_select {
            self.seat_states[idx] = SeatState::Selected;
            self.remove_from_available(idx);
        }

        Ok(())
    }

    /// Libera la reserva provisional de los asientos indicados (Selected → Available).
    ///
    /// Complejidad: O(k).
    pub fn deselect_seats(&mut self, seat_ids: &[String]) -> Result<(), String> {
        for seat_id in seat_ids {
            let idx = self.lookup(seat_id)?;
            if self.seat_states[idx] != SeatState::Selected {
                return Err(format!("Seat {} is not selected", seat_id));
            }
            self.seat_states[idx] = SeatState::Available;
            self.add_to_available(idx);
        }
        Ok(())
    }

    /// Confirma definitivamente la compra de los asientos indicados (Selected → Booked).
    ///
    /// Complejidad: O(k).
    pub fn book_seats(&mut self, seat_ids: &[String]) -> Result<Vec<String>, String> {
        // Validar que todos están Selected
        let mut indices: Vec<usize> = Vec::with_capacity(seat_ids.len());
        for seat_id in seat_ids {
            let idx = self.lookup(seat_id)?;
            match self.seat_states[idx] {
                SeatState::Selected => indices.push(idx),
                SeatState::Booked => {
                    return Err(format!("Seat {} is already booked", seat_id))
                }
                SeatState::Available => {
                    return Err(format!(
                        "Seat {} must be selected before booking",
                        seat_id
                    ))
                }
            }
        }

        // Transición Selected → Booked
        for &idx in &indices {
            self.seat_states[idx] = SeatState::Booked;
            self.n_booked += 1;
        }

        Ok(seat_ids.to_vec())
    }

    /// Reconstruye un `Sector` secuencial a partir del estado concurrente (para `get_venue()`).
    pub fn to_sector(&self) -> Sector {
        let seats = self
            .seat_ids
            .iter()
            .zip(self.seat_states.iter())
            .map(|(id, state)| Seat {
                id: id.clone(),
                state: state.clone(),
            })
            .collect();

        Sector {
            name: self.name.clone(),
            code: self.code.clone(),
            seats,
            capacity: self.capacity,
            is_numbered: self.is_numbered,
        }
    }
}
