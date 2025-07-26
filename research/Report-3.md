

# **Rust Embedding Solutions for Codanna's Tantivy-based Architecture: An Evaluation of Integration Strategies and Performance with pgvector**

## **I. Executive Summary**

This report provides a comprehensive analysis of Rust-based embedding generation and storage solutions pertinent to Codanna's Tantivy architecture. The evaluation focuses on identifying high-performance, memory-efficient, and scalable approaches, with a particular emphasis on integrating PostgreSQL with the pgvector extension.

The analysis indicates that Rust offers a robust ecosystem for machine learning and search infrastructure, leveraging its inherent memory safety, zero-cost abstractions, and concurrency features for high-throughput, low-latency operations. For embedding generation, libraries such as FastEmbed-rs and EmbedAnything stand out for their speed and memory efficiency, while ONNX Runtime (ort) and Burn provide broader model compatibility for specialized models like CodeBERT or CodeT5.

Tantivy, while a high-performance full-text search engine, does not natively support vector similarity search. Integrating vector capabilities requires either storing raw vectors in Tantivy's fast fields (inefficient for similarity search at scale) or, more practically, employing an external vector database. PostgreSQL with pgvector emerges as a highly competitive external solution. Recent benchmarks demonstrate that pgvector can outperform specialized vector databases like Pinecone in terms of latency and throughput for large datasets, all while leveraging PostgreSQL's mature transactional guarantees and cost-effectiveness. This positions pgvector as a primary recommendation for vector storage and search.

For Codanna, the most viable architectural path involves a hybrid approach: utilizing Tantivy for its core full-text search and filtering strengths, and pgvector as the authoritative, persistent store for vector embeddings. This combination benefits from pgvector's robust features, including iterative index scans for efficient hybrid search, and can be further optimized with Rust-native in-process caching for critical, low-latency query paths. Strategic recommendations include prioritizing quantized embedding models, optimizing pgvector parameters, and implementing content hashing for efficient incremental indexing. This approach offers a balance of performance, scalability, and operational simplicity, leveraging the strengths of both Rust and PostgreSQL.

## **II. Introduction to Embeddings and Vector Search in Rust**

### **The Role of Embeddings in Modern Search**

In contemporary search and information retrieval systems, vector embeddings have become a foundational component. These numerical representations transform discrete data types, such as text, images, or audio, into high-dimensional vectors that encapsulate their semantic meaning and contextual relationships.1 Unlike traditional keyword-based search, which relies on lexical matching, vector embeddings enable advanced search capabilities like semantic search, where queries can retrieve results based on meaning rather than exact word matches. This capability is crucial for applications such as Retrieval Augmented Generation (RAG) systems, where contextually relevant information must be efficiently retrieved to augment generative AI models.4 The transformation into numerical vectors allows for efficient comparison and grouping of similar concepts within a multi-dimensional space, facilitating tasks like similarity search, clustering, and recommendation systems.1

### **Why Rust for High-Performance ML and Search Infrastructure**

Rust has rapidly gained prominence in the development of high-performance machine learning (ML) and search infrastructure due to its unique combination of features. A primary advantage is its robust memory safety guarantees, which are enforced at compile-time through its ownership and borrowing system.5 This design fundamentally prevents common programming errors such as null pointer dereferences, buffer overflows, and data races, leading to more reliable and bug-free solutions compared to languages like C/C++.7

Furthermore, Rust offers zero-cost abstractions, meaning that high-level language features compile down to efficient machine code with no runtime overhead.5 This characteristic, combined with its lack of a garbage collector, ensures efficient memory management and predictable performance, which is critical for applications handling large datasets or requiring real-time processing.5 Rust's strong concurrency features enable developers to leverage multi-core processors effectively, facilitating high-throughput operations essential for modern ML workloads.5 The language's ability to simplify the transition from model training to deployment, often eliminating the need for code changes, further streamlines the deep learning workflow.10 These attributes collectively position Rust as an excellent choice for building high-performance, reliable, and scalable ML and search components.

### **Overview of the Report's Scope and Objectives for Codanna**

This report aims to provide Codanna with a comprehensive analysis of Rust-based solutions for generating and managing embeddings within their Tantivy-based architecture. The primary objectives include:

1. **Identification and Evaluation:** To identify and evaluate suitable Rust libraries and frameworks for generating embeddings, assessing their capabilities, performance, and suitability.  
2. **Tantivy Integration Assessment:** To assess how these embedding solutions can effectively integrate with Tantivy's existing architecture, considering its strengths, limitations, and potential extension points.  
3. **pgvector Evaluation:** To provide a detailed evaluation of PostgreSQL with the pgvector extension as a robust vector storage and search solution, with a specific focus on its integration within the Rust ecosystem.  
4. **Performance and Optimization:** To discuss critical aspects of performance, memory usage, and scalability, along with practical optimization techniques relevant to Rust embeddings and vector search.  
5. **Actionable Recommendations:** To offer clear, actionable recommendations for Codanna's implementation, balancing performance, scalability, and operational practicality.

The report maintains a consistent focus on leveraging Rust's performance and safety features while addressing the practical considerations for deployment and long-term maintenance.

## **III. Rust Libraries for Embedding Generation**

This section evaluates prominent Rust libraries and frameworks for generating embeddings, focusing on their capabilities, performance characteristics, and suitability for Codanna's needs.

### **Overview of Key Libraries**

* **Candle:** A minimalistic deep learning framework for Rust, designed for simplicity and high performance. Candle prioritizes computational efficiency by leveraging existing high-performance libraries and offering robust GPU support, including CUDA, Metal, and WebAssembly (WASM) for browser-based inference.11 It supports a variety of language models and text-to-text models like T5, Bert, and JinaBert, which are useful for sentence embeddings.13  
* **Burn:** A comprehensive deep learning framework in Rust that aims to provide a full machine learning stack. Burn offers greater flexibility and control over the ML pipeline, encompassing data loading, model definition, training, and hyperparameter optimization.11 It utilizes custom kernel code for computations, supports ONNX models, and provides various backends such as NdArray (for CPU), WGPU, Candle, and LibTorch, with advanced features like automatic kernel fusion and intelligent memory management.10  
* **FastEmbed-rs:** This library is specifically designed for rapid and efficient embedding generation, emphasizing high-speed performance and a low memory/disk footprint.1 It claims to be 50% faster than PyTorch Transformers and to offer better performance than Sentence Transformers and OpenAI Ada-002.19 A key advantage is its minimal dependencies, notably the absence of PyTorch or CUDA requirements for its CPU-optimized operations.19 FastEmbed-rs supports batch embedding generation and popular models like BAAI/bge and sentence-transformers.21  
* **Model2Vec-rs:** A Rust crate providing an efficient implementation for inference with Model2Vec static embedding models. It boasts high throughput, demonstrating approximately 1.7 times faster performance than its Python counterpart on a single-threaded CPU benchmark.11 Its tiny footprint and zero Python dependency make it highly suitable for semantic search and retrieval applications.22 The library supports  
  f32, f16, and i8 weight types, offering flexibility in precision.23  
* **ONNX Runtime (ort):** This is a Rust wrapper for Microsoft’s high-performance ONNX Runtime, which supports both CPU and GPU acceleration through various execution providers like CUDA, TensorRT, OpenVINO, and DirectML.11 It allows for loading ONNX models from files or directly from the ONNX Model Zoo, providing broad model compatibility for various AI tasks.25  
* **EmbedAnything:** A minimalist, yet highly performant, multisource, multimodal, and local embedding pipeline built in Rust.9 It streamlines the process of generating embeddings from diverse sources (text, images, audio, PDFs, websites) and supports dense, sparse, ONNX, and Model2Vec embeddings.9 A notable feature is "Vector Streaming," which enables memory-efficient indexing by processing data chunk-by-chunk using Rust's Multi-Producer Single-Consumer (MPSC) concurrency patterns, thereby eliminating the need for bulk embedding storage in RAM.9 It also offers GPU support via Candle.9

### **Comparative Analysis**

To facilitate an informed decision, Table 1 provides a comparative analysis of these Rust embedding generation libraries.

**Table 1: Rust Embedding Libraries Comparison**

| Feature / Library | Candle | Burn | FastEmbed-rs | Model2Vec-rs | ONNX Runtime (ort) | EmbedAnything |
| :---- | :---- | :---- | :---- | :---- | :---- | :---- |
| **Primary Focus** | High-perf DL | Comprehensive ML | Fast Inference | Static Embeddings | General Inference | Multi-source, Streaming Embeddings |
| **Performance/Throughput** | High (GPU) | High (GPU/CPU) | 50% faster than PyTorch, better than ST/Ada-002 19 | \~1.7x faster than Python on CPU 22 | High (GPU/CPU) | High (Rust-native, MPSC) 9 |
| **GPU/CPU Support** | CUDA, Metal, WASM, CPU 14 | CUDA, ROCm, Metal, Vulkan, WGPU, NdArray, LibTorch 11 | CPU optimized 19 | CPU optimized 22 | CPU, CUDA, TensorRT, OpenVINO, DirectML 24 | GPU via Candle, CPU 9 |
| **Model Compatibility** | LLMs (Llama, Mistral, Gemma), T5, Bert, JinaBert, Whisper, CLIP 13 | ONNX models, PyTorch/Safetensors import 11 | BAAI/bge, sentence-transformers 21 | Model2Vec static models 22 | Broad ONNX model compatibility 25 | BERT, Jina, CLIP, Splade, Model2Vec, Reranker, Qwen3-Embedding (HF/ONNX) 9 |
| **Memory Footprint** | Minimalistic API 12 | Intelligent memory management, no\_std for core 11 | Low memory/disk footprint, no PyTorch/CUDA deps 19 | Tiny footprint 22 | Efficient 25 | Low memory footprint, Vector Streaming 9 |
| **Key Features** | Model training, user-defined ops 14 | Automatic kernel fusion, asynchronous execution, training dashboard 11 | Batch processing, fast encodings 21 | Batch processing, configurable encoding, f16/i8 weights 23 | Graph optimization, execution providers 24 | Vector Streaming (MPSC), multimodal, dense/sparse/late-interaction 9 |
| **Dependencies** | Minimal | Flexible backends | No PyTorch/CUDA 19 | Zero Python dependency 22 | External C++ dependency (ONNX Runtime) 24 | No PyTorch dependency 9 |

### **Recommendations for Embedding Generation**

Based on the comparative analysis, specific recommendations for Codanna's embedding generation strategy can be made:

* **For High-Throughput, CPU-Centric Text Embedding Generation:** FastEmbed-rs and Model2Vec-rs are highly suitable. FastEmbed-rs offers exceptional speed and a low memory footprint without heavy dependencies, making it ideal for efficient, production-grade embedding generation.1 Model2Vec-rs provides similar advantages, particularly for static embedding models, with a demonstrated speed advantage over its Python counterpart.22  
* **For Broader Model Compatibility and GPU Acceleration:** If Codanna requires the flexibility to use a wider range of pre-trained models, including specialized ones like CodeBERT or CodeT5, or if GPU acceleration is a priority, ONNX Runtime (ort) and Burn are robust choices.15 These frameworks support ONNX models, which allows for importing models from other ecosystems (e.g., Hugging Face Transformers) after conversion.15  
* **For Large-Scale Document Processing and Multimodality:** EmbedAnything is a strong contender, especially if Codanna deals with large, diverse data sources (text, images, audio) and requires memory-efficient processing.9 Its "Vector Streaming" feature is particularly beneficial for handling large files without excessive RAM consumption, enabling continuous chunk-by-chunk embedding and direct streaming to a vector database.9  
* **For Deep Learning Model Deployment:** Candle, with its minimalistic API and strong GPU performance, is a good choice if Codanna's primary focus is on deploying specific deep learning models with high computational efficiency.12

The selection among these libraries should be guided by the specific types of models Codanna intends to use (e.g., general-purpose text embeddings vs. code-specific models), the importance of GPU acceleration, and the scale and nature of the input data.

## **IV. Tantivy's Architecture and Vector Integration Strategies**

This section delves into Tantivy's core architecture and explores how vector embeddings can be integrated, considering both its current design and community discussions.

### **Tantivy Fundamentals**

Tantivy is a high-performance full-text search engine library written in Rust, drawing significant inspiration from Apache Lucene.30 It is designed as a foundational component for building search engines, rather than an out-of-the-box server.30 The core design principles of Tantivy emphasize efficiency: search operations are designed to be O(1) in memory, and indexing operations are sublinear in practice.31

At its heart, a Tantivy index is composed of a collection of smaller, independent, and immutable segments.31 Each segment contains its own distinct set of data structures, and documents within a segment are identified by a compact

DocId, which is crucial for data compression.31 The storage mechanism is abstracted by the

Directory trait, with common implementations including MmapDirectory and RamDirectory.31

Tantivy enforces a strict schema for fields, defined prior to index creation. This schema dictates the types of fields (e.g., text, i64, u64, Date, JSON) and how they should be indexed or represented.31 Key data structures within Tantivy include:

* **Docstore:** This is a row-oriented storage mechanism for fields marked as "stored" in the schema. It is compressed using general-purpose algorithms like LZ4 and is typically used to retrieve and display search results.31  
* **Fast Fields:** These provide column-oriented storage, optimized for random access with bitpacking compression. They enable single memory fetches for value retrieval and can store raw byte payloads (&\[u8\]), which is relevant for storing vector features that might be used in advanced ranking models.30  
* **Incremental Indexing:** Tantivy supports adding new documents incrementally.30 However, a fundamental aspect of its design is data immutability; updating an existing document necessitates deleting the old document and reindexing the new version.30 Changes become searchable only after a  
  commit operation on an IndexWriter and are visible to newly acquired Searchers.30

### **Current Limitations for Native Vector Search**

Despite its strengths in full-text search, Tantivy is not inherently designed for vector similarity search or Approximate Nearest Neighbor (ANN) indexing. Its core mechanisms are built around BM25 scoring, inverted lists, and term dictionaries, which are optimized for lexical matching rather than vector space proximity.30 While Tantivy's

&\[u8\] fast fields can technically store raw vector data, the library does not provide any built-in indexing structures or query mechanisms (such as cosine similarity or L2 distance operators) to perform efficient vector similarity calculations directly on these fields.31 Consequently, any vector search operations on data stored in Tantivy's fast fields would require retrieving the vectors into the application layer and performing brute-force distance calculations, which is highly inefficient and impractical for large datasets.

### **Community Efforts and Roadmap**

The absence of native vector search capabilities in Tantivy has been a recognized need within its community, as evidenced by ongoing discussions on platforms like GitHub issue \#815.36 These discussions highlight a strong community interest in integrating vector similarity search directly into Tantivy. Proposed solutions often suggest incorporating external vector search libraries, such as Faiss, to enable the merging of traditional BM25 scores with vector distances at search time.36

The existence of such discussions underscores a significant missing feature in Tantivy's current functionality. This is further emphasized by the fact that some users, like NucliaDB, have developed sophisticated custom solutions to bridge this gap. NucliaDB, for instance, implemented its own ANN index (a variant of HNSW) that operates separately from Tantivy. This external index is built offline, loaded into memory at serving time, and then integrated with Tantivy through a custom AnnQuery implementation. This custom query leverages the warmed state of the ANN index for matching and scoring, allowing it to be combined with Tantivy's existing filtering clauses.36

The reliance on such complex custom implementations by production users, rather than a simple feature flag, indicates that a native, tightly integrated vector search capability within Tantivy is not readily available and likely represents a substantial architectural undertaking. The immutable nature of Tantivy's segments 31 and the requirement to delete and reindex documents for any updates 30 pose fundamental challenges to integrating dynamic vector indexes directly into its core data structures. Vector indexes, particularly HNSW, are inherently mutable, constantly updating their graph structures during insertions and deletions. Reconciling this dynamic nature with Tantivy's immutable segments would require significant design changes or complex synchronization mechanisms. This implies that Codanna cannot simply "enable" vector search within Tantivy; it must decide between building a sophisticated custom integration or adopting a robust external system. The "delete and reindex" process for updates is particularly problematic for frequently changing vector embeddings, as it can be computationally expensive to re-generate and re-index large vectors, potentially impacting real-time performance.

### **Proposed Integration Approaches for Codanna**

Given Tantivy's architecture and its current limitations regarding native vector search, Codanna has two primary integration approaches:

* **Option A: Storing Vectors in Tantivy's &\[u8\] Fast Fields:**  
  * **Feasibility:** It is technically possible to store raw vector data (e.g., a Vec\<f32\> converted to a byte slice &\[u8\]) within a Tantivy &\[u8\] fast field. This method would allow the vector embeddings to reside directly alongside other document metadata within Tantivy's index.31  
  * **Performance Implications:** While the retrieval of data from Tantivy's fast fields is highly efficient due to their column-oriented storage, random access, and bitpacking compression 31, Tantivy itself would not perform any vector similarity calculations or indexing on these byte arrays. All vector search operations, including distance calculations and Approximate Nearest Neighbor (ANN) searches, would need to be performed  
    *after* retrieving the vectors into the application's memory. This constitutes a brute-force approach, which is highly inefficient and computationally expensive for any dataset beyond a very small scale.  
  * **Challenges:** The primary challenge with this option is the complete lack of efficient similarity search capabilities within Tantivy. It would be practical only for scenarios where vector search is not a primary requirement, or for extremely small datasets where brute-force comparison is acceptable. This approach fails to leverage any specialized vector indexing algorithms (like HNSW) that are essential for performance at scale.  
* **Option B: External Vector Index with Tantivy for Filtering:**  
  * **Approach:** This strategy involves utilizing Tantivy for its core strengths—traditional full-text search, keyword filtering, and structured metadata filtering—while delegating vector similarity search to an *external* vector database or a dedicated in-process ANN library.  
  * **Workflow:** In a typical workflow, Tantivy would perform an initial search or filtering operation based on keywords or metadata, returning a subset of relevant document IDs. These IDs would then be used to retrieve corresponding vector embeddings from the external vector store, on which the similarity search would be executed. Alternatively, the vector search could be performed first on the external store, and the resulting top-k vector IDs then used to filter or re-rank results within Tantivy. The NucliaDB example, where a custom AnnQuery combines with Tantivy's filtering, illustrates this hybrid model.36  
  * **Advantages:** This approach leverages the specialized strengths of each system. Tantivy excels at full-text and structured filtering, while the external system provides optimized vector search. This separation allows each component to perform its core function efficiently.  
  * **Challenges:** The main challenges include managing two distinct data stores, ensuring data synchronization between them, and handling the potential complexity of merging results from two different search mechanisms. This dual-system architecture can introduce latency overhead due to inter-process communication or network calls between the Tantivy application and the external vector store.

For Codanna, Option B is generally the more scalable and performant choice for integrating vector search capabilities, as it leverages specialized tools for each task.

## **V. Evaluation of PostgreSQL with pgvector for Rust Integration**

This section provides a detailed evaluation of pgvector as a vector storage and search solution, specifically focusing on its integration within the Rust ecosystem.

### **pgvector Capabilities**

pgvector is a powerful open-source extension for PostgreSQL that transforms it into a capable vector database, allowing for the efficient storage and querying of high-dimensional vector data.2

* **Vector Data Type:** It introduces a native VECTOR data type, enabling direct storage of embeddings within PostgreSQL tables.2 This allows for similarity searches, clustering, and other machine learning tasks to be performed at scale.2  
* **Indexing:** pgvector supports two primary Approximate Nearest Neighbor (ANN) indexing algorithms to accelerate vector similarity searches: HNSW (Hierarchical Navigable Small World) and IVF Flat (Inverted File).37 HNSW graphs are widely recognized as among the top-performing indexes for vector similarity search.37  
* **Distance Functions:** It provides operators for common distance metrics, including Euclidean (L2) distance (\<-\>), Cosine distance (\<=\>), and Inner Product (\<\#\> or \-\#-).37  
* **Hybrid Search and Iterative Index Scans (pgvector 0.8.0+):** A significant enhancement in pgvector version 0.8.0 and later is the introduction of iterative index scanning for filtered vector searches.40 This feature directly addresses the "overfiltering" problem in previous versions, where SQL filters were applied  
  *after* the vector index scan, potentially leading to incomplete or no results.40 With iterative scanning,  
  pgvector incrementally scans the vector index and applies filters until the required number of results is found or a configurable limit is reached.40 This process significantly improves recall and performance for queries combining vector similarity with traditional SQL filters.40  
  * pgvector 0.8.0 offers strict\_order (preserving exact distance ordering) and relaxed\_order (approximate ordering for better performance) modes.40 For most production use cases,  
    relaxed\_order is recommended, as it provides substantial performance gains while typically maintaining 95-99% of result quality.40 This capability is critical for real-world applications like recommendation systems and semantic search, where combining vector relevance with metadata filtering is common.40  
* **Scalability:** When deployed on managed services like Amazon Aurora or RDS PostgreSQL, pgvector is capable of storing billions of vector embeddings and supporting high-performance native indexing for accelerated vector similarity search.37  
* **Dimensionality Support:** pgvector supports up to 16,000 vector dimensions, with vectors up to 2000 dimensions being easily indexed.37

### **Rust Integration Methods**

Integrating pgvector with Rust applications can be achieved through several methods, offering flexibility based on the project's needs for direct SQL interaction versus ORM abstraction.

* **Direct pgvector Crate Usage:** The pgvector crate provides direct Rust bindings for interacting with pgvector in PostgreSQL.39 This approach offers fine-grained control over SQL queries and vector operations. It supports both synchronous (  
  postgres feature) and asynchronous (sqlx feature) database clients.41 A common asynchronous choice is  
  tokio-postgres, which is an asynchronous, pipelined PostgreSQL client in Rust.43  
  tokio-postgres supports concurrent polling of futures for pipelined requests, which can significantly improve performance by minimizing idle time during multiple independent queries.43  
  * Example usage involves enabling the vector extension in PostgreSQL, creating tables with the VECTOR type, instantiating pgvector::Vector objects from Vec\<f32\>, inserting these vectors, and performing nearest neighbor queries using the \<-\> operator.39  
* **ORM Integration (SeaORM):** For applications preferring an Object-Relational Mapper (ORM), SeaORM is an asynchronous ORM for Rust that supports PostgreSQL.2 While SeaORM does not natively support the  
  VECTOR type, integration is achievable through a custom PgVector newtype and explicit casting in SQL queries.2  
  * This method requires annotating the model column referencing the VECTOR type with \#")\] and explicitly casting the VECTOR type to FLOAT4 within raw SQL queries when retrieving vector data via SeaORM.2 This ensures SeaORM can correctly map the PostgreSQL  
    VECTOR type to the Rust PgVector newtype.

### **Performance and Scalability Benchmarks**

The performance of pgvector has seen significant improvements, particularly with recent versions, and it demonstrates competitive capabilities against specialized vector databases.

* **Latency, Throughput, and Recall:**  
  * pgvector 0.8.0 offers substantial query performance improvements, with up to a 5.7x speedup for general query patterns and up to 9.4x for basic queries compared to version 0.7.4.40 This also translates to reduced latency, with typical e-commerce queries seeing runtimes decrease from over 120 milliseconds to just 70 milliseconds.40  
  * **Comparison with Pinecone:** Benchmarks conducted by TigerData comparing PostgreSQL with pgvector (and pgvectorscale, a TimescaleDB extension for PostgreSQL) against Pinecone for a large dataset of 50 million Cohere embeddings (768-dimensional) reveal compelling results.45  
    * Against Pinecone's storage-optimized index (s1) at 99% recall, PostgreSQL with pgvector and pgvectorscale achieved 28x lower p95 latency and 16x higher query throughput.45  
    * Against Pinecone's performance-optimized index (p2) at 90% recall, the PostgreSQL solution achieved 1.4x lower p95 latency and 1.5x higher query throughput.45  
    * These performance gains were also accompanied by a significant cost reduction, with self-hosting on AWS EC2 resulting in 75-79% lower monthly costs.45

The performance figures against Pinecone are particularly noteworthy. They challenge the common assumption that specialized vector databases inherently outperform general-purpose databases with vector extensions. For Codanna, this implies that pgvector is not merely a convenient option but a genuinely competitive, high-performance solution for handling large datasets. This is especially true when considering its cost-effectiveness and the substantial benefits derived from PostgreSQL's mature relational database management system (RDBMS) ecosystem, including robust transactional guarantees, comprehensive backup solutions, high availability features, and point-in-time recovery capabilities. This finding provides a strong basis for considering pgvector as a primary choice for vector storage and search.

* **Impact of Tuning Parameters:**  
  * hnsw.ef\_search: This parameter controls the width of the search in the lowest level of the HNSW graph. Higher values generally lead to more accurate results but at the cost of increased memory consumption and search time.40  
  * hnsw.iterative\_scan: This parameter is crucial for optimizing hybrid searches. The relaxed\_order mode is recommended for most production scenarios, as it provides a superior balance between performance and accuracy, offering minimal impact on result quality while significantly improving speed.40  
  * hnsw.max\_scan\_tuples and hnsw.scan\_mem\_multiplier: These configurable limits for iterative scanning allow fine-tuning the trade-off between result completeness and query performance.40

### **Transaction Consistency and Data Integrity**

A significant advantage of pgvector is its inherent benefit from PostgreSQL's strong ACID (Atomicity, Consistency, Isolation, Durability) compliance and robust transactional guarantees.37 This means that all operations involving vector data—inserts, updates, and deletes—are seamlessly integrated into standard database transactions. This ensures that data integrity is maintained even during concurrent operations or system failures.

The tokio-postgres crate in Rust provides a Transaction struct, which allows developers to explicitly manage database transactions with commit or rollback operations.43 This capability extends to vector operations, ensuring that a series of changes, including those to vector embeddings, are either fully applied or entirely discarded, thereby preventing partial updates or inconsistent states.44

This transactional integrity is a critical operational and reliability benefit. Many standalone vector databases or in-process Approximate Nearest Neighbor (ANN) libraries often lack such robust transactional guarantees, or they implement complex, eventually consistent models. For Codanna, relying on PostgreSQL's mature transaction system simplifies error handling, reduces the risk of data corruption, and makes recovery from failures more straightforward compared to managing consistency across disparate systems or custom in-memory indexes. This significantly reduces the development and operational burden associated with data management.

### **Memory Usage Estimates**

Understanding the memory footprint of vector embeddings and their associated indexes is crucial for capacity planning. Table 2 provides estimated memory usage for 1 million (1M) 768-dimensional vectors, including the overhead of an HNSW index, across different data types.

**Table 2: Estimated Memory Usage for 1M 768-Dimensional Vectors with HNSW Overhead**

| Data Type | Vector Data (Base) | HNSW Index Overhead (Approx.) | Total Estimated Memory |
| :---- | :---- | :---- | :---- |
| **Float32 (FP32)** | \~3 GB (1M \* 768 \* 4 bytes) 48 | \~128 MB (1M \* 32 connections \* 4 bytes) 48 | \~3.128 GB |
| **Int8 (Quantized)** | \~0.75 GB (750 MB) (1M \* 768 \* 1 byte) 49 | \~128 MB (if connections are 4-byte integers) | \~0.878 GB |
| **Binary (1-bit Quantized)** | \~91.55 MB (1M \* (768 / 8\) bytes) 53 | Potentially lower than 128 MB if optimized for binary vectors or smaller edge types 48 | \~91.55 MB \+ index overhead |

*Note: HNSW index overhead is calculated assuming M=32 connections per vector, with each connection stored as a 4-byte integer. Actual overhead may vary based on specific HNSW parameter tuning and implementation.*

This table highlights the significant memory savings achievable through quantization. For Codanna, these estimates are vital for making informed decisions about the trade-off between memory footprint and search accuracy. Quantization, by reducing the precision of vector components, can drastically decrease storage requirements and accelerate search operations, albeit with a slight approximation error.4 The table also illustrates the relatively fixed overhead introduced by HNSW indexing, regardless of the base vector data type, unless specific optimizations for edge storage are applied.

## **VI. Rust-Native Approximate Nearest Neighbor (ANN) Libraries**

This section explores standalone Rust libraries for Approximate Nearest Neighbor (ANN) search, which could be utilized in-process alongside Tantivy or as a component within a custom vector store.

### **Overview of Key Libraries**

* **hnsw\_rs:** This is a pure Rust implementation of the HNSW (Hierarchical Navigable Small World) algorithm, designed for efficient ANN search.46 It supports a wide array of distance metrics, including L1, L2, Cosine, Jaccard, Hamming, and Jensen-Shannon.46 The library offers multithreaded insertion and search capabilities and leverages SIMD acceleration for performance.46 For memory efficiency, it supports memory mapping for data and provides a "flattening conversion" to retain topology information with a low memory footprint.46  
  * **Benchmarks:** hnsw\_rs demonstrates strong performance metrics. For instance, on the fashion-mnist-784-euclidean dataset, it achieves 62,000 requests per second (req/s) with a recall rate of 0.977. For the sift1m benchmark (1 million 128-dimensional points), it can perform 15,000 req/s at 0.9907 recall for 10 nearest neighbors.46  
* **USearch:** Developed by Unum Cloud, USearch is a compact and high-performance similarity search engine written in Rust, aiming to surpass solutions like FAISS.61 It claims a 10x faster HNSW implementation compared to FAISS.61 USearch is highly optimized with SIMD instructions, supports  
  f16 (half-precision) and i8 (quarter-precision) data types, and is capable of handling large indexes directly from disk, which significantly minimizes RAM usage.61 Its memory efficiency is further enhanced through advanced downcasting and quantization techniques.61  
  * **Benchmarks:** USearch claims 10x faster indexing for 100 million 96-dimensional vectors compared to FAISS.61  
* **Hora:** Hora is an efficient approximate nearest neighbor search algorithm library implemented entirely in Rust.64 It features SIMD-acceleration and a multithreaded design for high performance.64 Hora supports multiple index types, including HNSW, Satellite System Graph (SSG), Product Quantization Inverted File (PQIVF), and BruteForce, along with various distance metrics.64 The library emphasizes reliability, with its code secured by the Rust compiler and memory managed by Rust's ownership system.64  
  * **Benchmarks:** Hora provides benchmark graphs, such as for fashion-mnist-784-euclidean, demonstrating its performance on AWS instances.64

### **Comparative Analysis**

To aid Codanna in selecting a suitable in-process ANN library, Table 3 provides a comparative analysis of hnsw\_rs, USearch, and Hora.

**Table 3: Rust-Native ANN Libraries Comparison**

| Feature / Library | hnsw\_rs | USearch | Hora |
| :---- | :---- | :---- | :---- |
| **Algorithms Supported** | HNSW 58 | HNSW 61 | HNSW, SSG, PQIVF, BruteForce, RPT (WIP) 64 |
| **Performance (Throughput/Recall)** | 62K req/s @ 0.977 recall (fashion-mnist-784-euclidean); 15K req/s @ 0.9907 recall (sift1m) 46 | Claims 10x faster HNSW than FAISS; 10x faster indexing for 100M 96-dim vectors 61 | Benchmarks available on AWS instances (e.g., fashion-mnist-784-euclidean) 64 |
| **Memory Efficiency Features** | Memory mapping, flattening conversion for low memory usage 46 | Disk-based indexing, f16/i8 support, downcasting, quantization 61 | no\_std (partial WIP), no heavy dependencies 64 |
| **SIMD/GPU Acceleration** | SIMD (simdeez\_f, stdsimd) 46 | SIMD, JIT compilation 61 | SIMD-accelerated (packed\_simd) 64 |
| **Maturity/Ecosystem** | Active development, C/Julia interface 46 | Multi-language support (C++, Python, JS, Java, Rust), fewer dependencies 61 | Multi-language support (Python, JS, Java, Go, Ruby, Swift, Julia WIP) 64 |
| **Key Differentiators** | Pure Rust, focus on HNSW, detailed benchmarks 46 | Claims FAISS superiority, disk-based indexing, advanced quantization 61 | Multiple index types, "ALL IN RUST" philosophy 64 |

### **Considerations for Codanna**

The decision to use an in-process Rust ANN library versus an external vector database like pgvector involves a critical trade-off between operational simplicity and data integrity on one hand, and absolute lowest latency and maximum control on the other.

Using an in-process ANN library offers the lowest possible latency for vector searches by eliminating network overhead, as computations occur within the same process.15 This approach also provides maximum control over the ANN algorithm, its memory management, and its integration points with Tantivy, allowing for highly customized solutions.

However, this comes with significant complexities. Codanna would become solely responsible for managing the persistence of the vector index (saving and loading it to/from disk), ensuring data consistency, and implementing robust fault tolerance and high availability mechanisms. These are complex engineering challenges that are typically abstracted away and handled by mature external databases. Furthermore, in-process indexes generally lack the robust transactional properties inherent to a full RDBMS like PostgreSQL. Data synchronization with Tantivy's immutable segments also becomes a more intricate task, as updates to documents in Tantivy require a delete-and-reindex operation, which would necessitate corresponding updates or rebuilds of the in-process vector index. While these libraries offer memory-efficient features like quantization and disk-based indexing, large indexes will still consume substantial RAM, requiring careful capacity planning.46

Considering the competitive benchmarks of pgvector against specialized vector databases 45, the performance gap between an external

pgvector solution and an in-process ANN library might not be substantial enough to justify the increased complexity for most use cases, especially when hybrid search capabilities are required. The operational overhead of building and maintaining a custom vector store, including managing persistence, consistency, and fault tolerance, would be considerable. This suggests that unless Codanna has very specific, sub-millisecond latency requirements for *pure* vector searches on frequently accessed "hot" data, and is willing to invest significant development effort in these operational aspects, the benefits of an in-process solution may not outweigh the added engineering burden.

## **VII. General Performance and Memory Optimization Strategies in Rust**

Beyond specific library choices, several general Rust-specific optimization techniques are crucial for maximizing the performance and minimizing the memory footprint of any embedding or vector search solution.

### **Rust's Memory Model**

Rust's distinctive ownership and borrowing system is a cornerstone of its performance and reliability.5 This compile-time memory safety mechanism prevents common memory-related errors without the overhead of a runtime garbage collector, contributing to high performance and predictable execution.5 By leveraging borrowing effectively, developers can avoid unnecessary data cloning, thereby conserving memory and processing power.6 Additionally, the

Drop trait provides a mechanism for explicit resource cleanup when objects go out of scope, ensuring that memory and other resources are returned to the system promptly.6

While Rust's memory safety is a fundamental advantage, it does not automatically guarantee a minimal memory footprint. Developers must remain deliberate in their choice of data structures and memory allocation patterns to achieve true efficiency. For instance, the Option\<T\> enum, while safe and ergonomic, can introduce non-constant memory overhead due to alignment requirements and "niche optimizations".71 This means that a seemingly small

Option\<bool\> might take 1 byte, but Option\<u64\> could take 8 bytes, not just 1 byte plus the size of u64.71 This behavior, while a result of Rust's safety guarantees and optimization for common cases (like null pointer optimization), illustrates that understanding Rust's memory layout rules is essential. To truly optimize for a low memory footprint, especially in resource-constrained environments or high-throughput embedding scenarios, developers need to actively select memory-efficient data structures (e.g.,

heapless collections for no\_std environments) and avoid unnecessary allocations or cloning.

### **Memory Allocators**

Rust's default memory allocator, often the system allocator (e.g., glibc malloc on Linux), can become a performance bottleneck, particularly under high concurrency due to global lock contention.72 In scenarios involving numerous concurrent memory allocations and deallocations, this can lead to reduced throughput and increased latency.

Swapping to modern, specialized memory allocators, such as jemalloc or mimalloc, can significantly improve performance.72 These allocators are designed to handle concurrent workloads more efficiently by reserving large memory arenas upfront and managing them in user space, thereby reducing the frequency of expensive kernel syscalls.72 The benefits of using a custom allocator include higher throughput, lower latency and costs, and improved stability by reducing memory fragmentation over long-running workloads.72 For high-performance, concurrent embedding generation and search workloads, Codanna should consider benchmarking and potentially integrating a custom memory allocator to mitigate potential bottlenecks.

### **Quantization Techniques**

Quantization is a critical optimization technique for reducing the memory footprint and accelerating computations of machine learning models and their embeddings.4 It involves reducing the precision of model parameters and embeddings, typically from 32-bit floating-point numbers to lower-precision data types like 8-bit integers (

i8) or even 1-bit binary representations.

Several types of quantization exist:

* **Scalar Quantization:** Converts 32-bit floats to 8-bit integers, achieving a 4x memory reduction.50 It can also speed up search by leveraging SIMD CPU instructions optimized for 8-bit integers.50  
* **Binary Quantization:** Represents each vector component with a single bit, leading to a 32x memory reduction and being the fastest method.50  
* **Product Quantization (PQ):** Divides vectors into chunks and quantizes each segment. While offering higher compression (up to 64x), it typically results in a significant loss of accuracy and is slower due to non-SIMD-friendly distance calculations.50

The primary trade-off with quantization is the introduction of approximation error, which can lead to a slight decrease in search quality.4 However, this error is often negligible for high-dimensional vectors. For example, FastEmbed's quantized models maintain a cosine similarity of 0.92 with original vectors.19 Burn supports static per-tensor quantization to

i8 (currently in beta) 17, and Qdrant offers various quantization options with configurable parameters for fine-tuning accuracy and memory/speed trade-offs.50

For Codanna, especially with large embedding datasets, quantization should be a primary optimization target to reduce storage and memory footprint and accelerate search operations. It is crucial to benchmark the accuracy versus performance trade-offs for Codanna's specific models and use cases to determine the optimal quantization level.

### **Dimensionality Reduction**

Dimensionality reduction techniques can further optimize memory usage and speed up distance calculations by transforming high-dimensional embeddings into a lower-dimensional space.73 Rust libraries like

linfa-reduction provide pure Rust implementations of various algorithms, including Principal Component Analysis (PCA), Diffusion Mapping, Gaussian random projections, and Sparse random projections.73 While reducing dimensionality introduces some information loss, it can be a valuable pre-processing step if memory is extremely constrained and a slight decrease in semantic fidelity is acceptable.

### **Batch Processing**

Processing multiple inputs (e.g., sentences, documents) in batches is a highly effective strategy for improving throughput during embedding generation, as it efficiently leverages parallel computation.9 Rust's robust concurrency features enable efficient asynchronous batching and "vector streaming," which reduces memory usage by processing data chunk-by-chunk rather than loading entire datasets into RAM.9 Libraries like FastEmbed-rs and EmbedAnything explicitly support and optimize for batch processing, making them suitable choices for high-volume embedding workloads.1 Codanna should prioritize embedding libraries and workflows that inherently support and optimize for batch processing and vector streaming to maximize overall efficiency.

### **Caching Strategies**

Implementing in-memory caching mechanisms can significantly boost the performance of embedding-related operations by storing frequently accessed embeddings or search results, thereby avoiding expensive re-computations or database queries.77 Common caching algorithms include Least Recently Used (LRU), Least Frequently Used (LFU), and Adaptive Replacement Cache (ARC).77

Rust offers powerful crates for building concurrent, thread-safe caches. For example, the moka crate provides highly concurrent caches inspired by Java's Caffeine library, with features such as size-aware eviction, time-to-live (TTL) and time-to-idle (TTI) expiration policies, and eviction listeners.78 For Codanna, a well-configured in-process cache (e.g., using

moka) can provide substantial latency improvements for "hot" data or repeated queries, complementing the persistence layer provided by pgvector.

### **Content Hashing**

For efficient change detection and data integrity checks in incremental indexing workflows, robust content hashing is essential.81 By computing a hash of the document content, Codanna can quickly determine if a document has been modified, triggering re-embedding and re-indexing only when necessary, rather than reprocessing all documents.86

Rust provides various hashing libraries beyond the standard library's default SipHash 1-3 (which is high-quality but slower for short keys).81 Alternatives include

rustc-hash (providing FxHashSet and FxHashMap with a very fast, low-quality FxHasher suitable for integer keys), fnv, and ahash (which can leverage AES instruction support for faster hashing on some processors).81 Profiling hashing performance and selecting an appropriate algorithm (e.g.,

FxHasher for speed, if HashDoS attacks are not a concern) is crucial for optimizing incremental indexing workflows.

## **VIII. Comparative Analysis and Recommendations for Codanna**

This section synthesizes the findings from the preceding analyses to provide actionable recommendations for Codanna, comparing the most viable architectural options for integrating vector search capabilities with their Tantivy-based system.

### **Option 1: Tantivy with External pgvector**

This approach leverages Tantivy for its core full-text search strengths while offloading vector storage and search to a dedicated PostgreSQL instance with the pgvector extension.

* **Advantages:**  
  * **Mature RDBMS Features:** This option benefits from PostgreSQL's robust data management capabilities, including ACID compliance, strong transactional integrity, comprehensive backup solutions, point-in-time recovery, and high availability features.37 These are critical for data reliability and operational stability.  
  * **Efficient Hybrid Search:** With pgvector 0.8.0 and its iterative index scans, the system can efficiently combine vector similarity search with traditional SQL filters.40 This is a crucial feature for many real-world search applications that require filtering by metadata alongside semantic relevance.  
  * **Competitive Performance:** Recent benchmarks demonstrate that pgvector (especially when combined with pgvectorscale) can outperform specialized vector databases like Pinecone in terms of latency and throughput for large datasets, often at a lower operational cost.45 This dispels the notion that a general-purpose database cannot compete in vector search.  
  * **Established Ecosystem:** PostgreSQL benefits from a large, active community, extensive tooling, and readily available managed services (e.g., AWS RDS/Aurora), simplifying deployment and maintenance.2  
  * **Simplified Architecture:** By extending an existing, well-understood RDBMS, Codanna can avoid introducing a new, specialized database technology into its stack, potentially simplifying overall system architecture, operations, and maintenance.  
* **Disadvantages:**  
  * **Inter-process Communication Overhead:** Network calls between the Tantivy application and the PostgreSQL database introduce a degree of latency that is unavoidable with an external system.  
  * **Less Fine-grained Control:** While configurable, pgvector offers less granular control over the underlying ANN index implementation compared to a custom in-process solution built from scratch.

### **Option 2: Tantivy with In-Process Rust ANN Library**

This approach involves integrating a Rust-native ANN library (e.g., hnsw\_rs, USearch, Hora) directly within Codanna's application process, alongside Tantivy.

* **Advantages:**  
  * **Lowest Latency:** Eliminating network overhead by performing vector search in-process offers the fastest possible query times for hot data.15  
  * **Maximum Control:** Codanna would have full control over the ANN algorithm, its memory management, and its integration points with Tantivy, allowing for highly customized solutions.  
  * **Single Codebase:** Potentially simpler deployment if the entire application can be packaged as a single Rust binary.  
  * **High Performance for Pure Vector Search:** These libraries are highly optimized for ANN search and can deliver exceptional performance for vector-only queries.46  
* **Disadvantages:**  
  * **Increased Complexity:** Codanna would assume significant responsibility for managing vector data persistence (saving/loading the index to disk), ensuring data consistency, implementing fault tolerance, and handling scaling, which are non-trivial tasks typically managed by external databases.  
  * **Lack of Transactional Guarantees:** In-process indexes generally lack the robust transactional properties of a full RDBMS, complicating data integrity during concurrent operations or failures.  
  * **Data Synchronization Challenges:** Careful design is required to keep the in-process vector index synchronized with Tantivy's document IDs and any updates (which necessitate delete-and-reindex in Tantivy). This can be a complex and error-prone process.  
  * **Memory Footprint:** While optimized, large in-memory indexes can still consume substantial RAM, especially for high-dimensional vectors, requiring careful resource provisioning.

### **Option 3: Hybrid Approach (Tantivy \+ pgvector \+ In-Process Caching/Optimization)**

This sophisticated approach combines the strengths of both external database and in-process solutions to achieve optimal performance and operational robustness.

* **Concept:** The hybrid model utilizes pgvector as the authoritative, persistent store for all vector embeddings, while introducing an in-process caching layer and potentially a lightweight in-process ANN index for frequently accessed "hot" data.  
* **Architecture:**  
  * **Tantivy:** Continues to serve as the primary engine for full-text search, keyword filtering, and structured metadata filtering.  
  * **pgvector:** Acts as the reliable, scalable backend for all vector embeddings. It leverages its HNSW indexing and hybrid search capabilities for filtered vector queries and less frequently accessed data.  
  * **In-Process Caching (Rust moka or lru):** A Rust-native cache (e.g., using moka for thread-safe, concurrent operations 78) stores frequently accessed embeddings and their associated Tantivy DocIds. This cache is populated from  
    pgvector on demand.  
  * **In-Process ANN (Optional):** For specific, extremely low-latency vector-only queries on the "hot" cached data, a lightweight in-process ANN index (e.g., built with hnsw\_rs, USearch, or Hora) can be maintained in memory.  
* **Advantages:**  
  * **Best of Both Worlds:** This approach combines pgvector's robustness, transactional integrity, and scalability with the low-latency performance of in-process components for critical query paths.  
  * **Tiered Scalability:** pgvector handles the large-scale persistence and complex filtered queries, while the caching and optional in-process ANN optimize for speed on frequently accessed data.  
  * **Flexibility:** Allows Codanna to fine-tune performance for different query types (e.g., keyword-only, hybrid, pure vector) and data access patterns.  
* **Disadvantages:**  
  * **Highest Complexity:** This architecture requires managing three distinct components (Tantivy, pgvector, and the custom in-process layer) and their intricate synchronization.  
  * **Cache Invalidation:** Implementing effective cache invalidation strategies (e.g., when embeddings are updated or deleted in pgvector) adds significant complexity and is crucial to prevent stale data.

### **Specific Recommendations for Codanna**

Based on the comprehensive analysis, the following specific recommendations are provided for Codanna:

* **Recommended Embedding Generation Libraries:**  
  * For general text embeddings, **FastEmbed-rs** is a strong candidate due to its CPU-optimized, high-throughput performance and minimal dependencies.1  
  * For multimodal data processing, large documents, or scenarios requiring memory-efficient streaming, **EmbedAnything** is recommended.9  
  * If the use of specific CodeBERT/CodeT5 models is essential, Codanna should ensure these models are converted to ONNX format, then utilize **ONNX Runtime (ort)** or **Burn** for efficient inference.15  
* **Optimal pgvector Configuration Parameters:**  
  * For pgvector 0.8.0 and above, it is recommended to enable hnsw.iterative\_scan \= 'relaxed\_order' for filtered queries. This setting provides an excellent balance between performance and recall, offering substantial speed gains with minimal impact on result quality.40  
  * The hnsw.ef\_search and hnsw.max\_scan\_tuples parameters should be carefully tuned based on Codanna's specific recall and latency targets, as well as the characteristics of their dataset.40  
  * To optimize hybrid queries, standard PostgreSQL indexes should be created on any columns used for SQL filtering alongside vector search.40  
* **Strategies for Memory Optimization and Quantization:**  
  * Prioritize the use of quantized embedding models (f16, i8, or binary) during embedding generation. This will significantly reduce the storage and memory footprint required by pgvector and any in-process caches.4  
  * Evaluate the accuracy trade-offs of different quantization levels for Codanna's specific use cases through empirical testing.  
  * If performance profiling indicates that the default memory allocator is a bottleneck under high concurrency, consider benchmarking and potentially swapping to a custom memory allocator (e.g., jemalloc).72  
* **Considerations for Incremental Indexing and Data Updates:**  
  * Given Tantivy's immutable nature for document updates (requiring a delete-and-reindex operation), Codanna's system will need a robust mechanism to manage this process for both text and corresponding vector embeddings.30  
  * Leverage pgvector's transactional capabilities to ensure atomicity and consistency during these update cycles, particularly when both text and vector data are modified.  
  * For any in-process ANN caches, implement effective cache invalidation or refresh strategies to accurately reflect updates originating from pgvector.  
  * Utilize efficient content hashing (e.g., using FxHasher for speed) to quickly detect changes in documents and trigger re-embedding and re-indexing only when absolutely necessary, thereby optimizing resource usage.81

## **IX. Conclusion**

The analysis firmly establishes that a robust and high-performance vector search solution for Codanna's Tantivy-based architecture is achievable within the Rust ecosystem. The most viable and strategically advantageous path forward involves a hybrid approach, leveraging the distinct strengths of Tantivy for full-text search and PostgreSQL with pgvector for scalable and reliable vector storage and search.

This recommended architecture offers significant strategic advantages. It capitalizes on pgvector's proven competitive performance against specialized vector databases, its inherent transactional integrity, and the operational simplicity derived from PostgreSQL's mature ecosystem. This mitigates the complexities and operational burden associated with managing a separate, specialized vector database. Furthermore, by integrating Rust-native embedding generation libraries and applying general Rust performance optimization strategies (such as quantization and efficient memory management), Codanna can achieve high throughput and low latency for embedding creation and retrieval. The option to introduce in-process caching for "hot" data provides an additional layer of optimization for critical, low-latency query paths, ensuring that Codanna's system can meet demanding performance requirements while maintaining data consistency and operational manageability.

Future Considerations:  
As Codanna's system evolves and scales, several areas warrant further exploration and development:

* **Deeper Tantivy Integration:** Monitor community efforts around native vector search integration within Tantivy. If these initiatives mature and offer robust, production-ready solutions, a re-evaluation of direct integration may be beneficial.  
* **Advanced Hybrid Search Techniques:** Explore more sophisticated hybrid search algorithms that dynamically weigh textual relevance from Tantivy and semantic relevance from pgvector based on query characteristics or user intent.  
* **Multi-modal Embeddings:** As data sources diversify, investigate advanced multi-modal embedding techniques and their efficient integration, particularly with libraries like EmbedAnything, to support richer search experiences.  
* **Continuous Performance Monitoring and Tuning:** Implement continuous monitoring of system performance, particularly for embedding generation, indexing, and search latency. This will allow for ongoing tuning of pgvector parameters, cache strategies, and resource allocation as Codanna's dataset scales and query patterns evolve.

#### **Works cited**

1. Qdrant — Using FastEmbed for Rapid Embedding Generation: A Benchmark and Guide | by Rayyan Shaikh | Medium, accessed July 25, 2025, [https://medium.com/@shaikhrayyan123/qdrant-using-fastembed-for-rapid-embedding-generation-a-benchmark-and-guide-dc105252c399](https://medium.com/@shaikhrayyan123/qdrant-using-fastembed-for-rapid-embedding-generation-a-benchmark-and-guide-dc105252c399)  
2. Using pgVector with SeaORM in Rust | Data Engineering ∪ Data Science, accessed July 25, 2025, [https://cosminsanda.com/posts/using-pgvector-with-seaorm-in-rust/](https://cosminsanda.com/posts/using-pgvector-with-seaorm-in-rust/)  
3. A Deep Dive into Qdrant, the Rust-Based Vector Database \- Analytics Vidhya, accessed July 25, 2025, [https://www.analyticsvidhya.com/blog/2023/11/a-deep-dive-into-qdrant-the-rust-based-vector-database/](https://www.analyticsvidhya.com/blog/2023/11/a-deep-dive-into-qdrant-the-rust-based-vector-database/)  
4. Optimizing Chunking, Embedding, and Vectorization for Retrieval-Augmented Generation | by Adnan Masood, PhD. | Medium, accessed July 25, 2025, [https://medium.com/@adnanmasood/optimizing-chunking-embedding-and-vectorization-for-retrieval-augmented-generation-ea3b083b68f7](https://medium.com/@adnanmasood/optimizing-chunking-embedding-and-vectorization-for-retrieval-augmented-generation-ea3b083b68f7)  
5. The Beginner's Guide to Machine Learning with Rust \- MachineLearningMastery.com, accessed July 25, 2025, [https://machinelearningmastery.com/the-beginners-guide-to-machine-learning-with-rust/](https://machinelearningmastery.com/the-beginners-guide-to-machine-learning-with-rust/)  
6. Advanced Rust Patterns for Embedded Programming \- Enhance Your Skills \- MoldStud, accessed July 25, 2025, [https://moldstud.com/articles/p-advanced-rust-patterns-for-embedded-programming-enhance-your-skills](https://moldstud.com/articles/p-advanced-rust-patterns-for-embedded-programming-enhance-your-skills)  
7. microflow: an efficient rust-based inference engine \- arXiv, accessed July 25, 2025, [https://arxiv.org/pdf/2409.19432?](https://arxiv.org/pdf/2409.19432)  
8. Machine Learning-Based Vulnerability Detection in Rust Code Using LLVM IR and Transformer Model \- Preprints.org, accessed July 25, 2025, [https://www.preprints.org/frontend/manuscript/ca734f823156d514cc7253ead760b337/download\_pub](https://www.preprints.org/frontend/manuscript/ca734f823156d514cc7253ead760b337/download_pub)  
9. StarlightSearch/EmbedAnything: Production-ready ... \- GitHub, accessed July 25, 2025, [https://github.com/StarlightSearch/EmbedAnything](https://github.com/StarlightSearch/EmbedAnything)  
10. tracel-ai/burn: Burn is a next generation Deep Learning Framework that doesn't compromise on flexibility, efficiency and portability. \- GitHub, accessed July 25, 2025, [https://github.com/tracel-ai/burn](https://github.com/tracel-ai/burn)  
11. burn-candle \- crates.io: Rust Package Registry, accessed July 25, 2025, [https://crates.io/crates/burn-candle](https://crates.io/crates/burn-candle)  
12. Candle vs Burn: Comparing Rust Machine Learning Frameworks ..., accessed July 25, 2025, [https://medium.com/@athan.seal/candle-vs-burn-comparing-rust-machine-learning-frameworks-4dbd59c332a1](https://medium.com/@athan.seal/candle-vs-burn-comparing-rust-machine-learning-frameworks-4dbd59c332a1)  
13. red-candle Native LLMs for Ruby, accessed July 25, 2025, [https://assaydepot.github.io/red-candle/](https://assaydepot.github.io/red-candle/)  
14. huggingface/candle: Minimalist ML framework for Rust \- GitHub, accessed July 25, 2025, [https://github.com/huggingface/candle](https://github.com/huggingface/candle)  
15. Building Your First AI Model Inference Engine in Rust | Nerds Support, Inc., accessed July 25, 2025, [https://nerdssupport.com/building-your-first-ai-model-inference-engine-in-rust/](https://nerdssupport.com/building-your-first-ai-model-inference-engine-in-rust/)  
16. Examples \- The Burn Book, accessed July 25, 2025, [https://burn.dev/burn-book/examples.html](https://burn.dev/burn-book/examples.html)  
17. Rust \- Burn.dev, accessed July 25, 2025, [https://burn.dev/docs/burn/](https://burn.dev/docs/burn/)  
18. Local Embeddings with Fastembed, Rig & Rust \- DEV Community, accessed July 25, 2025, [https://dev.to/joshmo\_dev/local-embeddings-with-fastembed-rig-rust-3581](https://dev.to/joshmo_dev/local-embeddings-with-fastembed-rig-rust-3581)  
19. FastEmbed: Qdrant's Efficient Python Library for Embedding Generation, accessed July 25, 2025, [https://qdrant.tech/articles/fastembed/](https://qdrant.tech/articles/fastembed/)  
20. Fastembed for Rig.rs \- Red And Green, accessed July 25, 2025, [https://redandgreen.co.uk/fastembed-for-rig-rs/rust-programming/](https://redandgreen.co.uk/fastembed-for-rig-rs/rust-programming/)  
21. fastembed \- crates.io: Rust Package Registry, accessed July 25, 2025, [https://crates.io/crates/fastembed/3.0.1](https://crates.io/crates/fastembed/3.0.1)  
22. Show HN: Model2vec-Rs – Fast Static Text Embeddings in Rust | Hacker News, accessed July 25, 2025, [https://news.ycombinator.com/item?id=44021883](https://news.ycombinator.com/item?id=44021883)  
23. MinishLab/model2vec-rs: Official Rust Implementation of ... \- GitHub, accessed July 25, 2025, [https://github.com/MinishLab/model2vec-rs](https://github.com/MinishLab/model2vec-rs)  
24. ort \- Rust bindings for ONNX Runtime \- Docs.rs, accessed July 25, 2025, [https://docs.rs/ort](https://docs.rs/ort)  
25. onnxruntime \- Rust \- Docs.rs, accessed July 25, 2025, [https://docs.rs/onnxruntime/latest/onnxruntime/](https://docs.rs/onnxruntime/latest/onnxruntime/)  
26. Vector Streaming: Memory-efficient Indexing with Rust \- Analytics Vidhya, accessed July 25, 2025, [https://www.analyticsvidhya.com/blog/2024/09/vector-streaming/](https://www.analyticsvidhya.com/blog/2024/09/vector-streaming/)  
27. embed\_anything \- Rust \- Docs.rs, accessed July 25, 2025, [https://docs.rs/embed\_anything](https://docs.rs/embed_anything)  
28. High memory usage when embedding large texts · Issue \#222 · qdrant/fastembed \- GitHub, accessed July 25, 2025, [https://github.com/qdrant/fastembed/issues/222](https://github.com/qdrant/fastembed/issues/222)  
29. Request: Add ONNX version of CodeT5+ or CodeBERT for browser usage · Issue \#1366 · huggingface/transformers.js \- GitHub, accessed July 25, 2025, [https://github.com/huggingface/transformers.js/issues/1366](https://github.com/huggingface/transformers.js/issues/1366)  
30. quickwit-oss/tantivy: Tantivy is a full-text search engine ... \- GitHub, accessed July 25, 2025, [https://github.com/quickwit-oss/tantivy](https://github.com/quickwit-oss/tantivy)  
31. tantivy/ARCHITECTURE.md at main · quickwit-oss/tantivy · GitHub, accessed July 25, 2025, [https://github.com/quickwit-oss/tantivy/blob/main/ARCHITECTURE.md](https://github.com/quickwit-oss/tantivy/blob/main/ARCHITECTURE.md)  
32. tantivy \- Rust \- Docs.rs, accessed July 25, 2025, [https://docs.rs/tantivy/](https://docs.rs/tantivy/)  
33. tantivy \- crates.io: Rust Package Registry, accessed July 25, 2025, [https://crates.io/crates/tantivy](https://crates.io/crates/tantivy)  
34. Tantivy 0.24 | Quickwit, accessed July 25, 2025, [https://quickwit.io/blog/tantivy-0.24](https://quickwit.io/blog/tantivy-0.24)  
35. Search, step-by-step · tantivy-doc, accessed July 25, 2025, [https://fulmicoton.gitbooks.io/tantivy-doc/content/step-by-step.html](https://fulmicoton.gitbooks.io/tantivy-doc/content/step-by-step.html)  
36. (Approximate) Nearest Neighbour / Vector Similarity Search · Issue ..., accessed July 25, 2025, [https://github.com/quickwit-oss/tantivy/issues/815](https://github.com/quickwit-oss/tantivy/issues/815)  
37. pgvector for AI-enabled PostgreSQL apps \- AWS, accessed July 25, 2025, [https://aws.amazon.com/awstv/watch/ed813f24f0a/](https://aws.amazon.com/awstv/watch/ed813f24f0a/)  
38. What's New in the Vector Similarity Search Extension? \- DuckDB, accessed July 25, 2025, [https://duckdb.org/2024/10/23/whats-new-in-the-vss-extension.html](https://duckdb.org/2024/10/23/whats-new-in-the-vss-extension.html)  
39. pgvector \- Rust \- Docs.rs, accessed July 25, 2025, [https://docs.rs/pgvector](https://docs.rs/pgvector)  
40. Supercharging vector search performance and relevance with ..., accessed July 25, 2025, [https://aws.amazon.com/blogs/database/supercharging-vector-search-performance-and-relevance-with-pgvector-0-8-0-on-amazon-aurora-postgresql/](https://aws.amazon.com/blogs/database/supercharging-vector-search-performance-and-relevance-with-pgvector-0-8-0-on-amazon-aurora-postgresql/)  
41. pgvector \- crates.io: Rust Package Registry, accessed July 25, 2025, [https://crates.io/crates/pgvector/0.1.4/dependencies](https://crates.io/crates/pgvector/0.1.4/dependencies)  
42. pgvector \- Rust \- Docs.rs, accessed July 25, 2025, [https://docs.rs/pgvector/latest/pgvector/](https://docs.rs/pgvector/latest/pgvector/)  
43. tokio\_postgres \- Rust \- Docs.rs, accessed July 25, 2025, [https://docs.rs/tokio-postgres](https://docs.rs/tokio-postgres)  
44. Transaction in tokio\_postgres \- Rust \- Docs.rs, accessed July 25, 2025, [https://docs.rs/tokio-postgres/latest/tokio\_postgres/struct.Transaction.html](https://docs.rs/tokio-postgres/latest/tokio_postgres/struct.Transaction.html)  
45. Pgvector vs. Pinecone: Vector Database Comparison | TigerData, accessed July 25, 2025, [https://www.tigerdata.com/blog/pgvector-vs-pinecone](https://www.tigerdata.com/blog/pgvector-vs-pinecone)  
46. hnsw\_rs \- crates.io: Rust Package Registry, accessed July 25, 2025, [https://crates.io/crates/hnsw\_rs](https://crates.io/crates/hnsw_rs)  
47. Transaction in postgres \- Rust \- Docs.rs, accessed July 25, 2025, [https://docs.rs/postgres/latest/postgres/struct.Transaction.html](https://docs.rs/postgres/latest/postgres/struct.Transaction.html)  
48. How much memory overhead is typically introduced by indexes like ..., accessed July 25, 2025, [https://milvus.io/ai-quick-reference/how-much-memory-overhead-is-typically-introduced-by-indexes-like-hnsw-or-ivf-for-a-given-number-of-vectors-and-how-can-this-overhead-be-managed-or-configured](https://milvus.io/ai-quick-reference/how-much-memory-overhead-is-typically-introduced-by-indexes-like-hnsw-or-ivf-for-a-given-number-of-vectors-and-how-can-this-overhead-be-managed-or-configured)  
49. Building a Vector Database: Everything You Should (But Often Don't ..., accessed July 25, 2025, [https://hiya31.medium.com/building-a-vector-database-everything-you-should-but-often-dont-consider-912db9783637?source=rss------artificial\_intelligence-5](https://hiya31.medium.com/building-a-vector-database-everything-you-should-but-often-dont-consider-912db9783637?source=rss------artificial_intelligence-5)  
50. Quantization \- Qdrant, accessed July 25, 2025, [https://qdrant.tech/documentation/guides/quantization/](https://qdrant.tech/documentation/guides/quantization/)  
51. microflow: an efficient rust-based inference engine \- arXiv, accessed July 25, 2025, [https://arxiv.org/pdf/2409.19432](https://arxiv.org/pdf/2409.19432)  
52. burn \- Rust \- Docs.rs, accessed July 25, 2025, [https://docs.rs/burn](https://docs.rs/burn)  
53. Thy are binary vectors with 768 dimensions, which takes up 96 bytes ..., accessed July 25, 2025, [https://news.ycombinator.com/item?id=40243906](https://news.ycombinator.com/item?id=40243906)  
54. Bring Vector Compression to the Extreme: How Milvus Serves 3× More Queries with RaBitQ, accessed July 25, 2025, [https://milvus.io/blog/bring-vector-compression-to-the-extreme-how-milvus-serves-3%C3%97-more-queries-with-rabitq.md](https://milvus.io/blog/bring-vector-compression-to-the-extreme-how-milvus-serves-3%C3%97-more-queries-with-rabitq.md)  
55. The best open-source embedding models | Baseten Blog, accessed July 25, 2025, [https://www.baseten.co/blog/the-best-open-source-embedding-models/](https://www.baseten.co/blog/the-best-open-source-embedding-models/)  
56. Evaluating Quantized Large Language Models for Code Generation on Low-Resource Language Benchmarks \- arXiv, accessed July 25, 2025, [https://arxiv.org/html/2410.14766v1](https://arxiv.org/html/2410.14766v1)  
57. kANNolo: Sweet and Smooth Approximate k-Nearest Neighbors Search \- arXiv, accessed July 25, 2025, [https://arxiv.org/html/2501.06121v1](https://arxiv.org/html/2501.06121v1)  
58. hnsw\_rs::hnsw \- Rust \- Docs.rs, accessed July 25, 2025, [https://docs.rs/hnsw\_rs/latest/hnsw\_rs/hnsw/index.html](https://docs.rs/hnsw_rs/latest/hnsw_rs/hnsw/index.html)  
59. hnsw\_rs \- Rust \- Docs.rs, accessed July 25, 2025, [https://docs.rs/hnsw\_rs](https://docs.rs/hnsw_rs)  
60. swapneel/hnsw-rust: HNSW implementation in Rust. Reference: https://arxiv.org/ftp/arxiv/papers/1603/1603.09320.pdf \- GitHub, accessed July 25, 2025, [https://github.com/swapneel/hnsw-rust](https://github.com/swapneel/hnsw-rust)  
61. USearch \- Rustfinity, accessed July 25, 2025, [https://www.rustfinity.com/open-source/usearch](https://www.rustfinity.com/open-source/usearch)  
62. usearch \- Rust \- Docs.rs, accessed July 25, 2025, [https://docs.rs/usearch](https://docs.rs/usearch)  
63. Why HNSW is not the answer and disk-based alternatives might be more practical | Hacker News, accessed July 25, 2025, [https://news.ycombinator.com/item?id=42496465](https://news.ycombinator.com/item?id=42496465)  
64. hora-search/hora: efficient approximate nearest neighbor ... \- GitHub, accessed July 25, 2025, [https://github.com/hora-search/hora](https://github.com/hora-search/hora)  
65. Qdrant \- Vector Database \- Qdrant, accessed July 25, 2025, [https://qdrant.tech/](https://qdrant.tech/)  
66. Vector Database Benchmarks \- Qdrant, accessed July 25, 2025, [https://qdrant.tech/benchmarks/](https://qdrant.tech/benchmarks/)  
67. Vector Search Performance Benchmark of SingleStore, Pinecone and Zilliz \- benchANT, accessed July 25, 2025, [https://benchant.com/blog/single-store-vector-vs-pinecone-zilliz-2025](https://benchant.com/blog/single-store-vector-vs-pinecone-zilliz-2025)  
68. Rust running on every GPU, accessed July 25, 2025, [https://rust-gpu.github.io/blog/2025/07/25/rust-on-every-gpu/](https://rust-gpu.github.io/blog/2025/07/25/rust-on-every-gpu/)  
69. Rust GPU: The future of GPU programming | Hacker News, accessed July 25, 2025, [https://news.ycombinator.com/item?id=41773096](https://news.ycombinator.com/item?id=41773096)  
70. Is your Vector Database Really Fast? \- DEV Community, accessed July 25, 2025, [https://dev.to/redis/is-your-vector-database-really-fast-i62](https://dev.to/redis/is-your-vector-database-really-fast-i62)  
71. Memory overhead of \`Option\` in Rust is not constant \[duplicate\] \- Stack Overflow, accessed July 25, 2025, [https://stackoverflow.com/questions/75783158/memory-overhead-of-option-in-rust-is-not-constant](https://stackoverflow.com/questions/75783158/memory-overhead-of-option-in-rust-is-not-constant)  
72. Double Your Performance with One Line of Code? The Memory Superpower Every Rust Developer Should Know\! \- DEV Community, accessed July 25, 2025, [https://dev.to/yeauty/double-your-performance-with-one-line-of-code-the-memory-superpower-every-rust-developer-should-1g93](https://dev.to/yeauty/double-your-performance-with-one-line-of-code-the-memory-superpower-every-rust-developer-should-1g93)  
73. linfa\_reduction \- Rust \- Docs.rs, accessed July 25, 2025, [https://docs.rs/linfa-reduction/](https://docs.rs/linfa-reduction/)  
74. linfa-reduction \- crates.io: Rust Package Registry, accessed July 25, 2025, [https://crates.io/crates/linfa-reduction](https://crates.io/crates/linfa-reduction)  
75. Pca in linfa\_reduction \- Rust, accessed July 25, 2025, [https://rust-ml.github.io/linfa/rustdocs/linfa\_reduction/struct.Pca.html](https://rust-ml.github.io/linfa/rustdocs/linfa_reduction/struct.Pca.html)  
76. Linfa Toolkit \- GitHub Pages, accessed July 25, 2025, [https://rust-ml.github.io/linfa/](https://rust-ml.github.io/linfa/)  
77. Building a Rust Caching System: A Step-by-Step Guide | by Codex \- Medium, accessed July 25, 2025, [https://medium.com/@emmaxcharles123/building-a-rust-caching-system-a-step-by-step-guide-8eda3912455d](https://medium.com/@emmaxcharles123/building-a-rust-caching-system-a-step-by-step-guide-8eda3912455d)  
78. moka-rs/moka: A high performance concurrent caching ... \- GitHub, accessed July 25, 2025, [https://github.com/moka-rs/moka](https://github.com/moka-rs/moka)  
79. Caching — list of Rust libraries/crates // Lib.rs, accessed July 25, 2025, [https://lib.rs/caching](https://lib.rs/caching)  
80. Master Caching in Rust (AWS & Redis): Impl., Lazy-Loading, Write-Through, Eviction, accessed July 25, 2025, [https://www.youtube.com/watch?v=oB3bom9dTew](https://www.youtube.com/watch?v=oB3bom9dTew)  
81. Hashing \- The Rust Performance Book, accessed July 25, 2025, [https://nnethercote.github.io/perf-book/hashing.html](https://nnethercote.github.io/perf-book/hashing.html)  
82. A brutally effective hash function in Rust | Nicholas Nethercote, accessed July 25, 2025, [https://nnethercote.github.io/2021/12/08/a-brutally-effective-hash-function-in-rust.html](https://nnethercote.github.io/2021/12/08/a-brutally-effective-hash-function-in-rust.html)  
83. Storing Keys with Associated Values in Hash Maps \- The Rust Programming Language, accessed July 25, 2025, [https://doc.rust-lang.org/beta/book/ch08-03-hash-maps.html](https://doc.rust-lang.org/beta/book/ch08-03-hash-maps.html)  
84. MetroHash in Rust \- SSOJet, accessed July 25, 2025, [https://ssojet.com/hashing/metrohash-in-rust/](https://ssojet.com/hashing/metrohash-in-rust/)  
85. Hashing and Validation of Fast-Hash in Rust Implementation \- MojoAuth, accessed July 25, 2025, [https://mojoauth.com/hashing/fast-hash-in-rust/](https://mojoauth.com/hashing/fast-hash-in-rust/)  
86. How does LlamaIndex support incremental indexing? \- Milvus, accessed July 25, 2025, [https://milvus.io/ai-quick-reference/how-does-llamaindex-support-incremental-indexing](https://milvus.io/ai-quick-reference/how-does-llamaindex-support-incremental-indexing)