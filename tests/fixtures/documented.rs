/// Processes a batch of items efficiently.
/// 
/// This function uses parallel processing for better performance
/// on large datasets. It's optimized for throughput over latency.
/// 
/// # Arguments
/// * `items` - A slice of items to process
/// 
/// # Returns
/// * `Result<ProcessedBatch, Error>` - The processed batch or an error
/// 
/// # Example
/// ```
/// let items = vec![Item::new(1), Item::new(2)];
/// let result = process_batch(&items)?;
/// ```
pub fn process_batch(items: &[Item]) -> Result<ProcessedBatch, Error> {
    // Implementation
    Ok(ProcessedBatch::default())
}

//// This is NOT a doc comment (4 slashes)
//// It should be ignored by our parser
fn not_documented() {
    println!("No docs here!");
}

/** Configuration structure for the processing system.
 * 
 * This struct holds all the runtime parameters needed
 * to configure the batch processor.
 */
pub struct Config {
    /// Maximum number of items to process in parallel
    pub max_parallel: usize,
    /// Timeout for each item in milliseconds  
    pub timeout_ms: u64,
}

/*** This is NOT a doc comment (3 asterisks) ***/
/*** It should be ignored ***/
fn also_not_documented() {
    // Regular function
}

/**/ // This is NOT a doc comment (empty 2-asterisk block)
fn edge_case_not_doc() {
    // Edge case
}

/// A simple point in 2D space
#[derive(Debug, Clone, Copy)]
pub struct Point {
    /// X coordinate
    pub x: f64,
    /// Y coordinate  
    pub y: f64,
}

impl Point {
    /// Creates a new point at the given coordinates.
    /// 
    /// # Arguments
    /// * `x` - The x coordinate
    /// * `y` - The y coordinate
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    
    /// Calculates the Euclidean distance to another point.
    /// 
    /// Uses the standard distance formula: √((x2-x1)² + (y2-y1)²)
    pub fn distance(&self, other: &Point) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }
}

/// Trait for shapes that can calculate their area
pub trait Area {
    /// Calculates and returns the area of the shape
    fn area(&self) -> f64;
}

/// Rectangle defined by two corner points
pub struct Rectangle {
    /// Top-left corner of the rectangle
    pub top_left: Point,
    /// Bottom-right corner of the rectangle  
    pub bottom_right: Point,
}

impl Area for Rectangle {
    /// Calculates the area of the rectangle.
    /// 
    /// Area = width × height
    fn area(&self) -> f64 {
        let width = (self.bottom_right.x - self.top_left.x).abs();
        let height = (self.bottom_right.y - self.top_left.y).abs();
        width * height
    }
}

/** A processor that handles items in batches.
 * 
 * This is a more complex example with multiple paragraphs
 * of documentation. The processor can be configured with
 * various options to control its behavior.
 * 
 * It supports both synchronous and asynchronous processing
 * modes, depending on the requirements.
 */
pub struct BatchProcessor {
    config: Config,
}

impl BatchProcessor {
    /** Creates a new batch processor with default configuration.
     * 
     * The default configuration uses:
     * - max_parallel: number of CPU cores
     * - timeout_ms: 5000
     */
    pub fn new() -> Self {
        Self {
            config: Config {
                max_parallel: num_cpus::get(),
                timeout_ms: 5000,
            }
        }
    }
    
    /// Processes a single batch with the current configuration
    pub fn process(&self, batch: &Batch) -> Result<(), Error> {
        // Processing logic here
        Ok(())
    }
}

// Test various doc comment edge cases
/// Empty doc comment on next line
///
fn empty_line_in_doc() {}

///Multiple
///lines
///without spaces  
fn compact_docs() {}

// Stub types for compilation
pub struct Item;
pub struct ProcessedBatch;
pub struct Batch;
pub struct Error;

impl Default for ProcessedBatch {
    fn default() -> Self { Self }
}

impl Item {
    pub fn new(_id: i32) -> Self { Self }
}

mod num_cpus {
    pub fn get() -> usize { 4 }
}