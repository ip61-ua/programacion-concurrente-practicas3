# Práctica 3: Venta de entradas concurrente en Rust

**Fecha límite de entrega:** 30 de noviembre de 2026  
**Lenguaje de trabajo:** Rust (`std::sync`, `std::thread`, atomics, mutexes)  
**Código de inicio:** 📥 **[p3.tgz](/p3.tgz)**

---

## 1. Objetivos de la práctica

1. **Diseñar e implementar una arquitectura de ticketing concurrente en Rust:** Transformar un sistema secuencial de venta de entradas en una solución multihilo de alto rendimiento capaz de atender ráfagas masivas de compradores concurrentes sobre recintos con diferentes sectores y capacidades.
2. **Garantizar la consistencia estricta (*Safety*):** Eliminar condiciones de carrera y asegurar la invariante de negocio fundamental: cero billetes duplicados, 100% de aforo vendido y ausencia de reservas huérfanas o perdidas.
3. **Minimizar la contención y maximizar el rendimiento (*Throughput*):** Identificar cuellos de botella generados por cerrojos globales de grano grueso y diseñar estrategias de sincronización eficientes en memoria compartida.
4. **Prevenir interbloqueos (*Deadlocks*) y pérdidas de vivacidad (*Liveness*):** Coordinar el ciclo de vida de los hilos compradores y la cola de espera para asegurar que el sistema progrese de forma continua y finalice limpiamente.

---

## 2. El problema: venta masiva de entradas y alta contención

Imagina el lanzamiento de entradas para un gran concierto o evento: miles de usuarios acceden simultáneamente para conseguir localidades en diferentes zonas del recinto (pistas de pie de admisión general, gradas con asientos numerados, etc.).

El problema es **intrínsecamente concurrente**: múltiples compradores consultan la disponibilidad, seleccionan asientos y confirman la compra al mismo tiempo. Sin los mecanismos de sincronización adecuados, aparecen graves problemas de concurrencia:
- **Condiciones de carrera (*Race Conditions*):** Venta duplicada del mismo asiento a dos usuarios distintos.
- **Inconsistencias y pérdidas de plazas:** Bloqueos temporales que nunca se liberan o entradas que quedan huérfanas.
- **Cuellos de botella por grano grueso:** Si se utiliza un único cerrojo global para todo el recinto, los hilos se serializan y el rendimiento colapsa.

> **Desacoplamiento Frontend-Backend:**  
> El sistema modela una arquitectura web cliente-servidor:
> * **Frontend / Cliente (`simulation.rs`):** Modela las sesiones concurrentes de los usuarios compradores (consulta de sectores, selección de butacas, confirmación o arrepentimiento). Interactúa con el backend exclusivamente mediante cadenas de texto (`String` / `&[String]`), simulando una API web.
> * **Backend / Motor de Reservas (`booking.rs`):** Implementa la lógica de disponibilidad, reserva provisional y venta definitiva. Debe garantizar la consistencia del estado del recinto en todo momento bajo acceso multihilo.

---

## 3. Código base de la práctica

El paquete inicial contiene la estructura del proyecto con la implementación secuencial de partida:
* `main.rs`: Punto de entrada de la aplicación (**no modificable**). Carga la configuración, inicializa el recinto y ejecuta la simulación.
* `config.rs`: Parámetros de ejecución y utilidades de configuración (**no modificable**).
* `simulation.rs`: Flujo de compra por sesión (**no modificable**). Orquesta el proceso de interacción de cada comprador.
* `booking.rs`: Define el **trait `Booker`** (la interfaz que debe cumplir el motor de reservas) y la implementación secuencial de referencia (`SequentialBooker`).
* `venue.rs`: Modelo de datos del recinto, sectores y asientos.
* `queue_manager.rs`: Define el **trait `QueueManager`** y el esqueleto para orquestar los hilos en `ConcurrentQueueManager`.
* `algorithm_name.txt`: Fichero de texto donde debes indicar el nombre o identificador de tu solución.

### Compilación y ejecución:
```bash
# 1. Compilar en modo release (imprescindible para medir rendimiento real)
cargo build --release

# 2. Ejecutar sobre el recinto de prueba incluido
cargo run --release
```

El código de partida incluye un recinto de ejemplo con sectores de admisión general y sectores numerados para que puedas probar tu solución en local. Durante la evaluación automática se utilizarán distintas configuraciones y tamaños de recinto para medir la robustez y escalabilidad de tu algoritmo.

---

## 4. Descripción del trabajo a realizar

Tu labor consiste en transformar el sistema secuencial en una **solución concurrente de alto rendimiento**, manteniendo la corrección estricta y cumpliendo el contrato de la API:

### 4.1 Implementar `ConcurrentQueueManager` (en `queue_manager.rs`)
- Debe implementar el trait `QueueManager`.
- Coordinar un conjunto de hilos *workers* (o un pool de tareas) para procesar concurrentemente las sesiones de compra de los usuarios que llegan a la cola.
- Recolectar de forma segura todos los identificadores de tickets vendidos por las distintas sesiones y devolverlos en el método:
  ```rust
  fn run(&mut self) -> Vec<String>
  ```

### 4.2 Implementar un `Booker` concurrente (en `booking.rs` o módulos adicionales)
- Debe implementar fielmente el trait `Booker`.
- Diseñar la sincronización interna para soportar acceso concurrente multihilo sin condiciones de carrera y minimizando la contención entre hilos.
- Puedes modificar `venue.rs` o añadir nuevos módulos auxiliares según el diseño de sincronización que elijas.

### 4.3 Flujo del proceso de compra por sesión (`simulation.rs`)
El módulo `simulation.rs` orquesta la simulación del comportamiento de cada comprador a través de los métodos de tu `Booker`:
1. **Consulta de sectores disponibles:** Llama a `get_available_sectors()` para conocer qué zonas disponen de aforo.
2. **Selección de sector:** El comprador elige un sector según sus preferencias e invoca `select_sector(&sector)`.
3. **Consulta de localidades disponibles:** Invoca `get_available_seats(&sector)` para obtener la lista de identificadores disponibles en dicho sector.
4. **Reserva provisional:** El comprador elige aleatoriamente $N$ localidades e intenta bloquearlas temporalmente llamando a `select_seats(&sector, &selected_seats)`.
5. **Decisión del usuario:**
   - Si el usuario **cambia de opinión** (50% de probabilidad), libera las localidades provisionales mediante `deselect_seats(&sector, &selected_seats)` y prueba con otro sector.
   - Si el usuario **confirma la compra**, invoca `book_seats(&sector, &selected_seats)`.
6. **Checkout definitivo:**
   - La llamada a `book_seats` confirma definitivamente la reserva y **debe devolver un `Result<Vec<String>, String>` con los identificadores únicos definitivos de las entradas vendidas**.
   - Si la compra no puede completarse (por ejemplo, porque la reserva provisional expiró o hubo un conflicto entre compradores), el usuario sufre una penalización de reintento y pasa a su siguiente preferencia.

---

## 5. Requisitos de la solución y restricciones

1. **Restricciones de modificación de código:**
   - **NO se pueden modificar:** `main.rs`, `simulation.rs` ni `config.rs`. Durante la evaluación automática se utilizarán las versiones oficiales de estos archivos.
   - **Puedes modificar y ampliar:** `booking.rs`, `queue_manager.rs`, `venue.rs` y crear los ficheros o submódulos auxiliares que requiera tu diseño.

2. **Prohibición de dependencias adicionales:**
   - **No se permite el uso de dependencias o *crates* externos adicionales.** Toda la sincronización y concurrencia debe implementarse haciendo uso exclusivo de la biblioteca estándar de Rust (`std::sync`, `std::thread`, atomics, mutexes, etc.). Durante la corrección automática se utilizará el entorno base oficial, por lo que cualquier dependencia externa introducida provocará un fallo de compilación.

3. **Condiciones de correctitud:**
   - **Cero ventas duplicadas:** Cada entrada vendida debe ser única.
   - **Venta completa:** Se debe vender exactamente el 100% de la capacidad del recinto.
   - **Ausencia de interbloqueos (*deadlocks*):** El programa debe finalizar de forma limpia y determinista dentro del tiempo establecido.

4. **Fichero de identificación del algoritmo:**
   - Debes incluir un fichero llamado `algorithm_name.txt` que contenga una única línea con el nombre que identifique tu solución o algoritmo.

---

## 6. Criterios de evaluación y entrega

La evaluación se realizará de forma **100% automática** sobre una batería de recintos de prueba con distintas configuraciones y niveles de concurrencia.

### Correctitud (Requisito indispensable)
En cada escenario de prueba se comprobarán estrictamente los siguientes requisitos:
- **Cero ventas duplicadas:** Ningún identificador de ticket puede haberse vendido más de una vez ($\text{len}(HashSet) == \text{len}(sold\_tickets)$).
- **Venta completa:** Se vende exactamente el 100% de la capacidad del recinto, sin localidades huérfanas ni bloqueos pendientes.
- **Ausencia de interbloqueos (*deadlocks*):** La ejecución debe finalizar limpiamente dentro del tiempo límite establecido.
- **Cumplimiento del contrato:** No se han alterado los ficheros protegidos (`main.rs`, `simulation.rs`, `config.rs`).

> **Importante:** Cualquier incumplimiento de las condiciones de correctitud en un escenario invalida la puntuación de dicho escenario.

### Rendimiento concurrente
Entre todas las entregas que superen los requisitos de correctitud en un escenario, la calificación se determinará a partir del **tiempo de ejecución y el rendimiento (*throughput*) obtenido**:
- Las implementaciones con menor tiempo de ejecución y menor contención obtendrán la máxima calificación.
- El uso de la solución secuencial básica o de soluciones concurrentes con serialización total (bloqueos globales de grano grueso) penalizará fuertemente la calificación, ya que el objetivo fundamental de la práctica es el diseño de algoritmos concurrentes eficientes.

### Formato y entrega
- La entrega se realiza a través de la plataforma [pracdlsi](https://pracdlsi.dlsi.ua.es/index.cgi).
- **No es necesario entregar `Cargo.toml`.**
- El archivo comprimido `p3.tgz` debe contener tu código fuente y tu fichero de identificación:
  - `booking.rs`
  - `queue_manager.rs`
  - `venue.rs` (y ficheros auxiliares propios, si los has creado)
  - `algorithm_name.txt`
- Para generar la entrega, coloca estos ficheros dentro de un directorio llamado `p3` y comprímelo mediante el terminal:
  ```bash
  tar cfz p3.tgz p3
  ```
- Puedes entregar tantas veces como desees antes de la fecha límite; el sistema evaluará únicamente la última entrega registrada.
