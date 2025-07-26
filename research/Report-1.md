# Research on Rust Embedding Solutions for Codanna’s Tantivy‑based System

## 1 Embedding generation libraries

### 1.1 Options available in the Rust ecosystem

| library                             | models / language support                                                                                                                                                                                                                                                                                                                                                                       | CPU/GPU & dependencies                                                                                                                                 | features                                                                                                                                                               | notes                                                                                                                                    |
| ----------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| **fastembed‑rs**                    | Provides ONNX‑based embeddings for many general text models (MiniLM, BGE series, nomic‑embed, Jina AI rerankers, etc.). Supports text, sparse text and image embeddings and returns 384‑ to 512‑dimensional `Vec<Vec<f32>>` vectors.                                                                                                                                                            | Uses `@pykeio/ort` (ONNX Runtime) and `hf‑tokenizers`, so no Python is required. Can run purely on CPU and optionally uses GPU via ONNX providers.     | Synchronous API; integrates easily into pure‑Rust code. Offers enumerated quantization modes (`None`, `Static`, `Dynamic`).                                            | Models are general‑purpose; no code‑specific models out‑of‑the‑box. Pre‑trained on natural language; quality on code may be lower.       |
| **Candle + CandleEmbed**            | Candle is a minimalist ML framework written in Rust. It emphasises high performance and GPU support. Candle’s examples list numerous code‑oriented LLMs: StarCoder, CodeGeeX4 and Replit‑code. `candle-embed` (CandleEmbed) wraps embedding models and allows users to select models (e.g., `all-MiniLM-L6`, `e5‑small`, etc.) and run them on CPU or CUDA with mean pooling and normalization. | Candle uses its own tensor engine; GPU support via CUDA and optional cuDNN. CandleEmbed downloads models from Hugging Face and runs inference locally. | Supports custom truncation, tokenization, and pooling strategies; can unload models to free memory.                                                                    | Since Candle runs full models (Transformers), memory usage is higher; however code‑oriented models (StarCoder, CodeGeeX4) are supported. |
| **rust‑bert with ONNX (via ort)**   | rust‑bert is a port of Hugging Face Transformers and supports sentence embeddings, translation, summarization, etc.. It can be compiled with an `onnx` feature which uses the `ort` crate for inference.                                                                                                                                                                                        | ONNX Runtime back‑end provides CPU and GPU execution.                                                                                                  | Offers ready‑to‑use pipelines and includes many language models but does not ship code‑specific models; however CodeBERT or CodeT5 ONNX models can be loaded manually. | Good for fine‑control of models, but heavier than fastembed; requires constructing tokenizers and pooling manually.                      |
| **sentence‑transformers‑burn**      | Implements BERT‑based sentence transformers on top of the Burn framework. Burn supports multiple back‑ends (Candle, LibTorch, WGPU) and hardware acceleration and is designed for high performance.                                                                                                                                                                                             | Supports CPU and GPU back‑ends; Burn provides asynchronous execution, kernel fusion and quantization.                                                  | Allows loading models saved in `safetensors`.                                                                                                                          | Code models are not provided; would require converting a code‑specific model to Burn format.                                             |
| **Custom ONNX inference via `ort`** | `ort` is a Rust binding to ONNX Runtime. It supports many execution providers (CUDA, TensorRT, OpenVINO, oneDNN) and automatically falls back to CPU when a provider is unavailable.                                                                                                                                                                                                            | Enables loading any ONNX model (e.g., CodeBERT, CodeT5 or StarCoder‑based embedding models) and running inference without Python.                      | Suitable when pre‑trained code‑embedding models are converted to ONNX. Requires manual tokenization and pooling; thus more engineering effort.                         |                                                                                                                                          |

### 1.2 Selecting an embedding model

* **General‑purpose vs. code‑specific models:** Fastembed’s default models (BGE, MiniLM) are trained on natural language and may not capture programming semantics. For code retrieval, models such as **CodeBERT**, **CodeT5**, **StarCoder** or **Replit‑code** provide better representations. These models can be executed in Rust via Candle or via ONNX using `ort`. Candle’s examples list code‑oriented LLMs such as *StarCoder* and *Replit‑code*, and the CLI examples illustrate that Candle can run them on CPU or CUDA. These models need large memory (several GB) but produce rich embeddings.
* **Embeddings dimension and memory:** Many transformer‑based embeddings have 768 or 1024 dimensions. At 32‑bit floats, a million embeddings would require 3 GB or more. To meet a 100 MB budget, dimensionality must be reduced or values quantized. For example, using a 128‑dimension embedding quantized to 8 bits reduces each vector to \~16 bytes plus overhead (roughly 16 MB for one million symbols). `fastembed` provides enumerated quantization modes (`Static`, `Dynamic`, `None`), though details are scant; the `usearch` index supports storing vectors in `f16` or `i8` formats and can operate on half‑ or quarter‑precision values. Alternatively, the *linfa‑reduction* crate offers PCA and random projection to reduce dimensionality to a lower number of floats.

### 1.3 Batching and incremental updates

Embedding generation is expensive. To achieve high throughput:

* **Batch inference:** Both `fastembed` and Candle support batch embedding. `fastembed` processes a batch of texts (default batch size 256) in one call. CandleEmbed exposes a builder that can embed a batch on GPU or CPU. For CodeBERT/CodeT5 models via `ort`, tokens can be batched before feeding into the model.
* **On‑the‑fly vs. pre‑processing:** During indexing, parse each file, collect code snippets (function definitions, classes, docstrings) and batch them for embedding. This ensures good CPU/GPU utilization. The current indexing speed (\~19 files/s) may drop if embeddings are computed sequentially; batching and GPU acceleration is required to approach tens of files per second.
* **Caching and incremental updates:** Use the existing `file_hash` field in the schema to detect unchanged files. When a file’s content hash matches the previous index, skip embedding generation. For changed files, remove the old embedding from the vector index and insert the new one. This ensures incremental updates without re‑embedding the entire corpus.

## 2 Vector storage and integration with Tantivy

### 2.1 Storing embeddings inside Tantivy

Tantivy supports binary or bytes fields. An embedding could be serialized to a `Vec<u8>` and stored in a `BytesOptions::STORED` field. Two approaches:

1. **Float32 bytes:** Serialize the `Vec<f32>` using `bytemuck` or `bincode` and store it as a binary blob. This retains full precision but consumes \~768 × 4 bytes per symbol (≈3 KB), which would far exceed the 100 MB budget for 1 M symbols. Retrieval also requires deserialization and computes similarity outside of Tantivy.
2. **Quantized/int8 bytes:** Quantize each vector to `i8` or `f16` before serialization. For example, projecting the embedding down to 128 dimensions and storing each component as an 8‑bit integer yields 128 bytes per symbol (≈128 MB for 1 M symbols). Additional metadata (min/max values for de‑quantization) can be stored alongside. This fits within the memory budget but reduces accuracy.

While storing embeddings in Tantivy keeps everything in one index and benefits from transactions, Tantivy currently lacks built‑in vector search. All similarity search must be implemented externally. The embedding field would mainly support result re‑ranking after a text search.

### 2.2 External vector index

Given Tantivy’s focus on text search, integrating a separate approximate nearest‑neighbour (ANN) index is a more scalable option. Two viable libraries are:

* **hnsw‑rs:** A pure‑Rust implementation of Hierarchical Navigable Small World graphs. It supports multiple distance metrics (L1, L2, cosine, Jaccard, Hellinger, Jensen‑Shannon) and allows custom metrics via traits. It supports multithreaded insertion and search, dumping/reloading the graph using Serde, and filtering search results via user‑supplied predicates. SIMD acceleration can be enabled for x86 \_64 through feature flags. The index can be memory‑mapped, which helps keep the data outside the main memory budget.
* **USearch:** A cross‑language search and clustering engine offering a highly optimized HNSW implementation. USearch claims to be **10× faster than FAISS** and supports half‑precision (`f16`) and quarter‑precision (`i8`) storage. It uses SIMD and JIT to support custom metrics, can view large indexes from disk without loading them entirely into RAM, and supports on‑the‑fly deletions and heterogeneous lookups. It also allows saving/loading indexes and memory‑mapping them, which reduces memory usage and startup time. Although written in C++, USearch has Rust bindings.

**Pros of a separate ANN index:**

* **Performance:** Ann libraries are optimized for vector search and can handle millions of vectors using HNSW graphs efficiently. USearch’s comparison with FAISS shows dramatically shorter indexing times for 96‑ and 1536‑dimension vectors (0.2–2.1 hours vs. 2.6–5 hours).
* **Memory control:** Both hnsw‑rs and USearch support quantized storage (f16 or i8). USearch allows using `f16` or `i8` storage directly in the index.
* **Filtering:** hnsw‑rs supports filtering functions evaluated during search, enabling hybrid operations (e.g., restrict by module or file).
* **Concurrency:** hnsw‑rs supports multithreaded insertion and queries, fitting the concurrent indexing pipeline.

**Cons:** Extra complexity: the developer must maintain two indices (Tantivy for text, ANN for vectors) and ensure they remain in sync. However, the separation allows independent tuning and reduces coupling.

### 2.3 Integration strategy

1. **Extend the schema:** Add an `embedding` field to the Tantivy `IndexSchema` with `BytesOptions::STORED` for persistence. Store a quantized representation of the vector (e.g., 128 dimensions × 8 bits). The original high‑precision vector lives only in the vector index. This provides a fallback for re‑ranking or debugging.
2. **Maintain an ANN index:** During indexing, after generating each embedding, insert it into the HNSW (hnsw‑rs or USearch) index with the `symbol_id` as the key. Keep the index on disk using memory‑mapped files. For incremental updates, remove the old entry and insert the new one.
3. **Hybrid search pipeline:**

   * **Text search:** For a query string, use Tantivy’s `QueryParser` to perform full‑text search across `name` and `doc_comment`. Retrieve a pool of top‑N candidates (e.g., 100).
   * **Vector search:** Generate the query embedding using the same model and search the ANN index to get K nearest neighbours.
   * **Merge:** Intersect the two result sets. For candidates present in both, combine scores: normalized BM25 score from Tantivy and cosine similarity from the vector index (perhaps weighted). For documents returned only by vector search, optionally include if their similarity is high.
   * **Return final ranking:** Sort by combined score and fetch full document data from Tantivy via the `symbol_id` field.

The hybrid approach leverages the strengths of both retrieval methods: textual relevance and semantic similarity.

## 3 Handling incremental updates

* **Batch embedding:** During indexing, collect code elements into batches (size 64–256) before passing them to the embedding model. On a CPU, `fastembed` can embed 256 sentences in one call; on GPU, Candle can process larger batches.
* **Detect unchanged files:** Use the existing `file_hash` to skip embedding unchanged files. When a file is unchanged, skip both Tantivy update and ANN update.
* **Updating the ANN index:** hnsw‑rs allows deletions by marking an id for removal and reinserting; USearch supports on‑the‑fly deletions. When a symbol is deleted or renamed, remove its vector from the index and update the mapping. Batch updates can be applied by building a new index offline and swapping pointers.
* **Transaction consistency:** In the existing `IndexWriter` batch commit, after all documents are indexed and commit succeeds, flush the vector updates to the ANN index and persist the index file. Use the same transaction boundary to ensure both indices are in a consistent state.

## 4 Performance and memory considerations

| dimension & precision     | storage per vector | memory for 1M vectors | notes                                                                                  |
| ------------------------- | ------------------ | --------------------- | -------------------------------------------------------------------------------------- |
| 768 floats (f32)          | 768×4 = 3 kB       | \~3 GB                | highest quality but violates budget.                                                   |
| 384 floats (f32)          | 1.5 kB             | \~1.5 GB              | still too large.                                                                       |
| **128 floats (f32)**      | **512 B**          | **\~512 MB**          | applying PCA/random projection using `linfa‑reduction` reduces dimension; still heavy. |
| **128 half‑floats (f16)** | **256 B**          | **\~256 MB**          | reduces memory; half‑precision supported by USearch index.                             |
| **128 quantized (i8)**    | **128 B**          | **\~128 MB**          | near target; storing quantization parameters adds small overhead.                      |
| **64 quantized (i8)**     | **64 B**           | **\~64 MB**           | further dimension reduction; lower accuracy.                                           |

To stay within the \~100 MB budget, embedding dimension must be ≤128 with int8 quantization. `linfa-reduction` can apply random projection or PCA to 128 dimensions. The quantized vector can then be stored in the ANN index using the `i8` or `b1` types supported by USearch.

## 5 Recommended architecture

1. **Embedding generation**

   * Use `fastembed` as the default embedding generator because it runs on CPU, requires minimal dependencies, and exposes a simple synchronous API. For better code semantics, investigate converting a **code‑specific model** (e.g., CodeBERT or CodeT5) to ONNX and load it with the `ort` crate. Candle can be used when GPU acceleration is available; its support for code models like StarCoder and CodeGeeX4 means future upgrades can add contextual embeddings.
   * Use batch processing. Tune batch size (64–256) to balance memory use and throughput. Generate embeddings during the parsing phase of the indexer.
2. **Vector storage**

   * Add a `embedding` bytes field to Tantivy for each symbol; store a quantized version (e.g., 128 × i8). Provide a small header with min/max scaling factors if dynamic quantization is used.
   * Build a separate HNSW index using **hnsw‑rs** or **USearch**. Insert each symbol’s high‑precision embedding (f32 or f16) into the graph. For memory efficiency, store the vectors in `f16` or `i8`. Use the `symbol_id` as the external label. Persist the ANN index to disk after each commit.
3. **Hybrid search pipeline**

```
pub fn hybrid_search(&self, query: &str) -> Result<Vec<Symbol>> {
    // 1. Text search (BM25) in Tantivy
    let query_parser = QueryParser::for_index(&self.index, vec![self.schema.name, self.schema.doc_comment]);
    let text_query = query_parser.parse_query(query)?;
    let top_docs = self.searcher.search(&text_query, &TopDocs::with_limit(200))?;

    // 2. Generate query embedding
    let mut embedder = fastembed::TextEmbedding::try_new(Default::default())?;
    let query_embedding = embedder.embed_single(format!("query: {}", query))?;

    // 3. Vector search
    let ann_results = self.ann_index.search(&query_embedding, 100)?; // returns Vec<(symbol_id, distance)>

    // 4. Merge scores
    let mut candidates = HashMap::new();
    for (score, doc_address) in top_docs {
        let symbol_id = self.doc_store.symbol_id(doc_address)?;
        candidates.insert(symbol_id, (score as f32, None));
    }
    for (symbol_id, sim) in ann_results {
        candidates.entry(symbol_id).and_modify(|e| e.1 = Some(sim)).or_insert((0.0, Some(sim)));
    }

    // 5. Re‑rank and collect
    let mut scored: Vec<(f32, Symbol)> = candidates
        .iter()
        .map(|(&symbol_id, &(bm25, opt_sim))| {
            let vector_score = opt_sim.unwrap_or(0.0);
            let combined = 0.7 * bm25 + 0.3 * vector_score; // weight parameters
            let symbol = self.doc_store.get_symbol(symbol_id)?;
            Ok((combined, symbol))
        })
        .collect::<Result<_>>()?;
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    Ok(scored.into_iter().take(10).map(|(_, sym)| sym).collect())
}
```

4. **Incremental updates and persistence**

   * During indexing, compute a `file_hash` to detect unchanged files. For new or changed symbols, generate embeddings and update the ANN index. For deleted symbols, remove their embedding from the ANN index (both hnsw‑rs and USearch support deletions).
   * Keep the ANN index on disk; memory‑map it on startup. USearch allows viewing large indexes from disk without loading them fully.
   * Commit both Tantivy and ANN index within the same transaction boundary to ensure consistency.

5. **Future considerations**

   * **Tantivy native vector support:** there is currently no official vector field in Tantivy. The community has discussed vector search, but the recommended approach is to use an external index.
   * **Quantization and compression:** evaluate quantization schemes (e.g., product quantization, scalar quantization) for further memory savings. USearch supports storing vectors as `i8` and `b1` types.
   * **Scaling:** For multi‑tenant or large repositories, consider sharding the ANN index across multiple processes or using USearch’s ability to map indexes without loading into RAM to reduce memory footprint.

## Conclusion and migration plan

1. **Prototype with fastembed:** Add `fastembed` to the build and generate 384‑dimensional embeddings. Integrate hnsw‑rs as an ANN index and implement the hybrid search pipeline. Evaluate indexing throughput and query latency.
2. **Dimensionality reduction:** Use `linfa-reduction` (PCA/random projection) to reduce embeddings to 128 dimensions and quantize to `i8` to fit the memory budget.
3. **Evaluate code‑specific models:** Convert CodeBERT or CodeT5 to ONNX and benchmark inference via `ort`. Compare retrieval quality against general models. If GPU is available, experiment with Candle’s StarCoder or Replit‑code models for higher‑quality embeddings.
4. **Deploy incremental updates:** Extend the existing indexing pipeline to generate and update embeddings. Use file hashes for caching and ensure both Tantivy and ANN index commits occur atomically.
5. **Monitor memory and performance:** Track memory usage for the ANN index and adjust dimensionality/quantization accordingly. Evaluate search latency; hnsw‑rs and USearch both provide multi‑threaded search and can handle millions of vectors efficiently.

This architecture allows Codanna to integrate semantic search without abandoning Tantivy’s efficient text indexing. By selecting appropriate embedding models, leveraging ANN libraries for vector search, and reducing the dimensionality of embeddings, the system can remain memory‑efficient while providing high‑quality code retrieval.
