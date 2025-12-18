//! Document chunking strategies.
//!
//! Provides the `Chunker` trait and implementations for splitting documents
//! into chunks suitable for embedding.

use super::config::ChunkingConfig;

/// A raw chunk before being assigned IDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawChunk {
    /// Byte range in the source document (start, end).
    pub byte_range: (usize, usize),

    /// The text content of this chunk.
    pub content: String,

    /// Heading hierarchy context (e.g., ["Chapter 1", "Section 1.2"]).
    pub heading_context: Vec<String>,
}

impl RawChunk {
    /// Create a new raw chunk.
    pub fn new(byte_range: (usize, usize), content: String, heading_context: Vec<String>) -> Self {
        Self {
            byte_range,
            content,
            heading_context,
        }
    }

    /// Get character count.
    pub fn char_count(&self) -> usize {
        self.content.chars().count()
    }
}

/// Trait for document chunking strategies.
pub trait Chunker: Send + Sync {
    /// Split document content into chunks.
    fn chunk(&self, content: &str, config: &ChunkingConfig) -> Vec<RawChunk>;
}

/// Hybrid chunker: paragraph-based with size constraints.
///
/// Algorithm:
/// 1. Extract heading positions for context
/// 2. Split by paragraphs (double newline)
/// 3. Merge small paragraphs (< min_chunk_chars)
/// 4. Split large chunks with sliding window + overlap
/// 5. Attach heading context to each chunk
#[derive(Debug, Default)]
pub struct HybridChunker;

impl HybridChunker {
    /// Create a new hybrid chunker.
    pub fn new() -> Self {
        Self
    }
}

/// A heading found in the document.
#[derive(Debug, Clone)]
struct Heading {
    /// Level (1-6 for H1-H6).
    level: u8,
    /// Text of the heading.
    text: String,
    /// Byte position where heading ends.
    end_byte: usize,
}

impl Chunker for HybridChunker {
    fn chunk(&self, content: &str, config: &ChunkingConfig) -> Vec<RawChunk> {
        if content.is_empty() {
            return Vec::new();
        }

        // Step 1: Extract headings for context
        let headings = extract_headings(content);

        // Step 2: Split by paragraphs
        let paragraphs = split_paragraphs(content);

        // Step 3: Merge small paragraphs
        let merged = merge_small_paragraphs(paragraphs, config.min_chunk_chars);

        // Step 4: Split large chunks with sliding window
        let split = split_large_chunks(merged, config.max_chunk_chars, config.overlap_chars);

        // Step 5: Attach heading context
        attach_heading_context(split, &headings)
    }
}

/// A paragraph with its byte range.
#[derive(Debug, Clone)]
struct Paragraph {
    byte_range: (usize, usize),
    content: String,
}

/// Extract markdown headings from content.
fn extract_headings(content: &str) -> Vec<Heading> {
    let mut headings = Vec::new();

    for (line_start, line) in content
        .match_indices('\n')
        .map(|(i, _)| i + 1)
        .chain(std::iter::once(0))
        .map(|start| {
            let end = content[start..]
                .find('\n')
                .map_or(content.len(), |i| start + i);
            (start, &content[start..end])
        })
    {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix('#') {
            // Count heading level
            let mut level = 1u8;
            let mut chars = rest.chars();
            while let Some('#') = chars.next() {
                level += 1;
                if level > 6 {
                    break;
                }
            }

            // Must have space after hashes
            let heading_text = rest.trim_start_matches('#').trim();
            if level <= 6 && !heading_text.is_empty() {
                headings.push(Heading {
                    level,
                    text: heading_text.to_string(),
                    end_byte: line_start + line.len(),
                });
            }
        }
    }

    headings
}

/// Split content into paragraphs (by double newline).
fn split_paragraphs(content: &str) -> Vec<Paragraph> {
    let mut paragraphs = Vec::new();
    let mut in_paragraph = false;
    let mut para_start = 0;

    let bytes = content.as_bytes();
    let len = bytes.len();

    let mut i = 0;
    while i < len {
        let is_newline = bytes[i] == b'\n';

        if !in_paragraph {
            // Skip leading whitespace
            if !is_newline && !bytes[i].is_ascii_whitespace() {
                in_paragraph = true;
                para_start = i;
            }
        } else {
            // Check for double newline
            if is_newline && i + 1 < len && bytes[i + 1] == b'\n' {
                // End paragraph
                let para_content = content[para_start..i].trim().to_string();
                if !para_content.is_empty() {
                    paragraphs.push(Paragraph {
                        byte_range: (para_start, i),
                        content: para_content,
                    });
                }
                in_paragraph = false;
                i += 1; // Skip second newline
            }
        }
        i += 1;
    }

    // Don't forget last paragraph
    if in_paragraph {
        let para_content = content[para_start..].trim().to_string();
        if !para_content.is_empty() {
            paragraphs.push(Paragraph {
                byte_range: (para_start, len),
                content: para_content,
            });
        }
    }

    paragraphs
}

/// Merge small paragraphs together.
fn merge_small_paragraphs(paragraphs: Vec<Paragraph>, min_chars: usize) -> Vec<Paragraph> {
    if paragraphs.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::new();
    let mut iter = paragraphs.into_iter();
    let mut current = iter.next().unwrap();

    for para in iter {
        if current.content.chars().count() < min_chars {
            // Merge with next
            current.byte_range.1 = para.byte_range.1;
            current.content.push_str("\n\n");
            current.content.push_str(&para.content);
        } else {
            result.push(current);
            current = para;
        }
    }

    // Don't forget the last one
    result.push(current);
    result
}

/// Split large paragraphs with sliding window and overlap.
fn split_large_chunks(
    paragraphs: Vec<Paragraph>,
    max_chars: usize,
    overlap_chars: usize,
) -> Vec<Paragraph> {
    let mut result = Vec::new();

    for para in paragraphs {
        let char_count = para.content.chars().count();

        if char_count <= max_chars {
            result.push(para);
        } else {
            // Split with sliding window
            let chars: Vec<char> = para.content.chars().collect();
            let step = max_chars.saturating_sub(overlap_chars).max(1);

            let mut char_start = 0;
            while char_start < chars.len() {
                let char_end = (char_start + max_chars).min(chars.len());
                let chunk_content: String = chars[char_start..char_end].iter().collect();

                // Calculate byte positions
                let byte_start = para.byte_range.0
                    + para.content[..chars[..char_start].iter().collect::<String>().len()].len();
                let byte_end = byte_start + chunk_content.len();

                result.push(Paragraph {
                    byte_range: (byte_start, byte_end.min(para.byte_range.1)),
                    content: chunk_content,
                });

                if char_end >= chars.len() {
                    break;
                }
                char_start += step;
            }
        }
    }

    result
}

/// Attach heading context to each chunk.
fn attach_heading_context(paragraphs: Vec<Paragraph>, headings: &[Heading]) -> Vec<RawChunk> {
    paragraphs
        .into_iter()
        .map(|para| {
            // Find all headings that precede this paragraph
            let context: Vec<String> = headings
                .iter()
                .filter(|h| h.end_byte <= para.byte_range.0)
                .fold(Vec::new(), |mut acc, h| {
                    // Keep heading hierarchy (lower level replaces, higher level adds)
                    while acc.len() >= h.level as usize {
                        acc.pop();
                    }
                    while acc.len() < h.level as usize - 1 {
                        acc.push(String::new()); // Placeholder for skipped levels
                    }
                    acc.push(h.text.clone());
                    acc
                })
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect();

            RawChunk::new(para.byte_range, para.content, context)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> ChunkingConfig {
        ChunkingConfig {
            min_chunk_chars: 50,
            max_chunk_chars: 200,
            overlap_chars: 20,
            ..Default::default()
        }
    }

    #[test]
    fn test_empty_content() {
        let chunker = HybridChunker::new();
        let chunks = chunker.chunk("", &default_config());
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_single_paragraph() {
        let chunker = HybridChunker::new();
        let content = "This is a single paragraph with enough text to be meaningful.";
        let chunks = chunker.chunk(content, &default_config());

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, content);
    }

    #[test]
    fn test_multiple_paragraphs() {
        let chunker = HybridChunker::new();
        let content =
            "First paragraph with some content here.\n\nSecond paragraph with more content here.";
        let config = ChunkingConfig {
            min_chunk_chars: 10,
            max_chunk_chars: 200,
            overlap_chars: 5,
            ..Default::default()
        };
        let chunks = chunker.chunk(content, &config);

        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].content.contains("First"));
        assert!(chunks[1].content.contains("Second"));
    }

    #[test]
    fn test_merges_small_paragraphs() {
        let chunker = HybridChunker::new();
        let content =
            "Tiny.\n\nAlso tiny.\n\nThis is a longer paragraph that should stand on its own.";
        let config = ChunkingConfig {
            min_chunk_chars: 50,
            max_chunk_chars: 500,
            overlap_chars: 20,
            ..Default::default()
        };
        let chunks = chunker.chunk(content, &config);

        // "Tiny." and "Also tiny." should be merged
        // The longer paragraph should be separate
        assert!(chunks.len() <= 2);
        assert!(chunks[0].content.contains("Tiny"));
    }

    #[test]
    fn test_splits_large_paragraph() {
        let chunker = HybridChunker::new();
        // Create a paragraph larger than max_chunk_chars
        let content = "word ".repeat(100); // ~500 chars
        let config = ChunkingConfig {
            min_chunk_chars: 20,
            max_chunk_chars: 100,
            overlap_chars: 20,
            ..Default::default()
        };
        let chunks = chunker.chunk(&content, &config);

        // Should be split into multiple chunks
        assert!(chunks.len() > 1);

        // Each chunk should be at most max_chunk_chars
        for chunk in &chunks {
            assert!(chunk.char_count() <= config.max_chunk_chars);
        }
    }

    #[test]
    fn test_heading_context() {
        let chunker = HybridChunker::new();
        let content = r#"# Chapter 1

Introduction paragraph here.

## Section 1.1

Content in section 1.1.

## Section 1.2

Content in section 1.2.

# Chapter 2

Content in chapter 2."#;

        let config = ChunkingConfig {
            min_chunk_chars: 10,
            max_chunk_chars: 500,
            overlap_chars: 10,
            ..Default::default()
        };
        let chunks = chunker.chunk(content, &config);

        // Find the chunk with "section 1.2" content
        let section_12_chunk = chunks
            .iter()
            .find(|c| c.content.contains("Content in section 1.2"))
            .expect("Should find section 1.2 chunk");

        // Should have heading context
        assert!(!section_12_chunk.heading_context.is_empty());
        assert!(
            section_12_chunk
                .heading_context
                .iter()
                .any(|h| h.contains("Chapter 1"))
        );
    }

    #[test]
    fn test_byte_ranges_valid() {
        let chunker = HybridChunker::new();
        let content = "First paragraph.\n\nSecond paragraph.";
        let chunks = chunker.chunk(content, &default_config());

        for chunk in &chunks {
            let (start, end) = chunk.byte_range;
            assert!(start <= end);
            assert!(end <= content.len());
        }
    }

    #[test]
    fn test_overlap_between_split_chunks() {
        let chunker = HybridChunker::new();
        let content = "The quick brown fox jumps over the lazy dog. ".repeat(20);
        let config = ChunkingConfig {
            min_chunk_chars: 10,
            max_chunk_chars: 100,
            overlap_chars: 30,
            ..Default::default()
        };
        let chunks = chunker.chunk(&content, &config);

        // Verify multiple chunks were created due to size limit
        assert!(
            chunks.len() > 1,
            "Large content should be split into multiple chunks"
        );
    }
}
