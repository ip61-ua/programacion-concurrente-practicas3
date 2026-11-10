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
// Estado interno concurrente por sector (Opción C)
// ============================================================

/// Estado mutable protegido por `Mutex` para un sector concreto.
///
/// Mantiene un índice O(1) de asientos disponibles mediante swap-pop,
/// evitando scans lineales en sectores grandes (ej. Standing: 12.500 asientos).
pub struct SectorState {
    /// Nombre descriptivo del sector.
    pub name: String,
    /// Código corto del sector (ej. "ST", "GC", "S1").
    pub code: String,
    /// ¿Los asientos son numerados?
    pub is_numbered: bool,
    /// IDs de todos los asientos del sector (inmutable tras construcción).
    pub seat_ids: Vec<String>,
    /// Estado de cada asiento, indexado por posición en `seat_ids`.
    pub seat_states: Vec<SeatState>,
    /// Índices (en `seat_ids`) de los asientos actualmente `Available`.
    /// Se mantiene como pila desordenada: pop/push O(1).
    pub available: Vec<usize>,
    /// Número de asientos en estado `Booked` en este sector.
    pub n_booked: usize,
    /// Capacidad total del sector.
    pub capacity: usize,
}

impl SectorState {
    /// Construye el estado concurrente de un sector a partir del modelo secuencial.
    pub fn from_sector(sector: &Sector) -> Self {
        let seat_ids: Vec<String> = sector.seats.iter().map(|s| s.id.clone()).collect();
        let seat_states: Vec<SeatState> = sector.seats.iter().map(|s| s.state.clone()).collect();
        // Inicialmente todos los asientos están Available
        let available: Vec<usize> = (0..seat_ids.len()).collect();

        SectorState {
            name: sector.name.clone(),
            code: sector.code.clone(),
            is_numbered: sector.is_numbered,
            capacity: sector.capacity,
            seat_ids,
            seat_states,
            available,
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
    pub fn n_booked(&self) -> usize {
        self.n_booked
    }

    /// Devuelve los IDs de todos los asientos actualmente disponibles.
    pub fn available_seat_ids(&self) -> Vec<String> {
        self.available.iter().map(|&i| self.seat_ids[i].clone()).collect()
    }

    /// Intenta marcar los asientos indicados como `Selected` (reserva provisional).
    ///
    /// Si algún asiento no está `Available`, deshace los cambios parciales y retorna `Err`.
    /// Complejidad: O(k) donde k = número de asientos solicitados.
    pub fn select_seats(&mut self, seat_ids: &[String]) -> Result<(), String> {
        // Encontrar los índices de los asientos solicitados y validarlos
        let mut indices_to_select: Vec<usize> = Vec::with_capacity(seat_ids.len());
        for seat_id in seat_ids {
            match self.seat_ids.iter().position(|id| id == seat_id) {
                Some(idx) => {
                    if self.seat_states[idx] != SeatState::Available {
                        return Err(format!("Seat {} is not available", seat_id));
                    }
                    indices_to_select.push(idx);
                }
                None => return Err(format!("Seat {} not found in sector {}", seat_id, self.code)),
            }
        }

        // Transición Available → Selected y actualizar el índice de disponibles
        for &idx in &indices_to_select {
            self.seat_states[idx] = SeatState::Selected;
            // Eliminar del vec de disponibles: swap con el último + pop → O(1)
            if let Some(pos) = self.available.iter().position(|&i| i == idx) {
                let last = self.available.len() - 1;
                self.available.swap(pos, last);
                self.available.pop();
            }
        }

        Ok(())
    }

    /// Libera la reserva provisional de los asientos indicados (Selected → Available).
    ///
    /// Complejidad: O(k).
    pub fn deselect_seats(&mut self, seat_ids: &[String]) -> Result<(), String> {
        for seat_id in seat_ids {
            match self.seat_ids.iter().position(|id| id == seat_id) {
                Some(idx) => {
                    if self.seat_states[idx] != SeatState::Selected {
                        return Err(format!("Seat {} is not selected", seat_id));
                    }
                    self.seat_states[idx] = SeatState::Available;
                    self.available.push(idx);
                }
                None => return Err(format!("Seat {} not found in sector {}", seat_id, self.code)),
            }
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
            match self.seat_ids.iter().position(|id| id == seat_id) {
                Some(idx) => {
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
                None => return Err(format!("Seat {} not found in sector {}", seat_id, self.code)),
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
