;; Comprehensive Clojure test file for parser maturity assessment.
;; Tests all major Clojure language features and constructs.

;; --- Namespace declaration with requires and imports ---

(ns my.app.core
  "Application core namespace with data processing utilities."
  (:require [clojure.string :as str]
            [clojure.set :refer [union intersection difference]]
            [clojure.walk :as walk]
            [clojure.java.io :as io])
  (:import [java.util Date UUID]
           [java.io File IOException]
           [java.time Instant Duration]))

;; --- Constants and variables ---

(def ^:const MAX_RETRIES
  "Maximum number of retry attempts before giving up."
  3)

(def ^:const BATCH_SIZE
  "Number of items to process in each batch."
  100)

(def default-config
  "Default configuration map for the application."
  {:host "localhost"
   :port 8080
   :debug false
   :max-connections 50})

(def ^:private internal-state
  "Mutable internal state atom, not exposed to consumers."
  (atom {}))

(def ^:private -registry
  "Private registry for tracking registered handlers."
  (atom []))

(def ^:dynamic *debug-mode*
  "Dynamic var controlling debug output. Bind to true for verbose logging."
  false)

(def ^:dynamic *current-user*
  "Dynamic var holding the current authenticated user context."
  nil)

(def ^{:doc "Application version string" :private true} version "0.9.15")

(def supported-formats
  "Set of file formats supported for import and export."
  #{:json :edn :csv :xml})

;; --- Public functions ---

(defn validate-input
  "Validate input data against required field constraints.
   Returns the data if valid, throws on invalid input."
  [data required-fields]
  (let [missing (remove #(contains? data %) required-fields)]
    (when (seq missing)
      (throw (ex-info "Missing required fields"
                      {:missing missing :provided (keys data)})))
    data))

(defn process-batch
  "Process a batch of records with the given transform function.
   Supports multiple arities."
  ([records]
   (process-batch records identity))
  ([records transform-fn]
   (process-batch records transform-fn BATCH_SIZE))
  ([records transform-fn batch-size]
   (->> records
        (partition-all batch-size)
        (mapcat #(map transform-fn %)))))

(defn merge-configs
  "Merge multiple configuration maps with left-to-right precedence."
  [& configs]
  (reduce (fn [acc cfg]
            (merge-with (fn [old new] (if (nil? new) old new))
                        acc cfg))
          {}
          configs))

(defn retry-with-backoff
  "Execute a function with exponential backoff retry logic."
  [f & {:keys [max-retries delay-ms]
        :or {max-retries MAX_RETRIES delay-ms 100}}]
  (loop [attempt 1]
    (let [result (try
                   {:ok (f)}
                   (catch Exception e {:error e}))]
      (if (:ok result)
        (:ok result)
        (if (>= attempt max-retries)
          (throw (:error result))
          (do
            (Thread/sleep (* delay-ms (Math/pow 2 (dec attempt))))
            (recur (inc attempt))))))))

(defn deep-merge
  "Recursively merge nested maps. Non-map values are replaced."
  [& maps]
  (apply merge-with
         (fn [v1 v2]
           (if (and (map? v1) (map? v2))
             (deep-merge v1 v2)
             v2))
         maps))

(defn transform-keys
  "Transform all keys in a map using the provided function."
  [f m]
  (into {}
        (map (fn [[k v]]
               [(f k) (if (map? v) (transform-keys f v) v)]))
        m))

;; --- Private functions ---

(defn- normalize-record
  "Normalize a data record by trimming strings and lowering keys."
  [record]
  (into {}
        (map (fn [[k v]]
               [(-> k name str/lower-case keyword)
                (if (string? v) (str/trim v) v)]))
        record))

(defn- calculate-checksum
  "Calculate a simple checksum for data integrity verification."
  [data]
  (reduce + (map hash (vals data))))

(defn- format-error-message
  "Format an error message with context for logging."
  [error context]
  (str "[" (:operation context) "] "
       (.getMessage error)
       " (attempt " (:attempt context 0) ")"))

;; --- Higher-order functions and closures ---

(defn make-validator
  "Create a validation function from a map of field names to predicates."
  [rules]
  (fn [record]
    (reduce-kv
     (fn [errors field pred]
       (if (pred (get record field))
         errors
         (conj errors {:field field :value (get record field)})))
     []
     rules)))

(defn make-rate-limiter
  "Create a rate limiter that allows n calls per interval-ms."
  [n interval-ms]
  (let [calls (atom [])]
    (fn []
      (let [now (System/currentTimeMillis)
            cutoff (- now interval-ms)]
        (swap! calls (fn [cs] (filterv #(> % cutoff) cs)))
        (if (< (count @calls) n)
          (do (swap! calls conj now) true)
          false)))))

;; --- Protocols ---

(defprotocol Renderable
  "Protocol for entities that can be rendered to a string."
  (render [this] "Render the entity to a string.")
  (render-with-options [this options] "Render with the given option map."))

(defprotocol Cacheable
  "Protocol for entities that support caching behavior."
  (cache-key [this] "Return a unique cache key for this entity.")
  (ttl [this] "Return the time-to-live in seconds.")
  (serialize [this] "Serialize the entity for cache storage."))

(defprotocol Lifecycle
  "Protocol for components with start/stop lifecycle management."
  (start [this] "Start the component and return the started instance.")
  (stop [this] "Stop the component and release resources."))

;; --- Records implementing protocols ---

;; Record representing a circle with radius and center coordinates.
(defrecord Circle [radius center-x center-y]
  Renderable
  (render [this]
    (str "Circle(r=" (:radius this)
         " at " (:center-x this) "," (:center-y this) ")"))
  (render-with-options [this options]
    (if (:verbose options)
      (str "Circle[radius=" (:radius this)
           ", area=" (* Math/PI (:radius this) (:radius this)) "]")
      (render this))))

;; Record representing a rectangle with dimensions and position.
(defrecord Rectangle [width height x y]
  Renderable
  (render [this]
    (str "Rect(" (:width this) "x" (:height this) ")"))
  (render-with-options [this options]
    (if (:verbose options)
      (str "Rectangle[area=" (* (:width this) (:height this)) "]")
      (render this))))

;; Record representing a cache entry with key, value, and expiration.
(defrecord CacheEntry [key value created-at ttl-seconds]
  Cacheable
  (cache-key [_] key)
  (ttl [_] ttl-seconds)
  (serialize [this]
    (pr-str {:key key :value value :created-at created-at})))

;; --- deftype ---

;; Mutable buffer type backed by a Java ArrayList.
(deftype MutableBuffer [^:volatile-mutable ^java.util.ArrayList items]
  Renderable
  (render [_]
    (str "MutableBuffer(size=" (.size items) ")"))
  (render-with-options [this options]
    (if (:show-contents options)
      (str "MutableBuffer" (vec items))
      (render this))))

;; --- Multimethods ---

(defmulti shape-area
  "Calculate the area of a shape based on its :type key."
  :type)

(defmethod shape-area :circle
  "Calculate the area of a circle from its radius."
  [{:keys [radius]}]
  (* Math/PI radius radius))

(defmethod shape-area :rectangle
  "Calculate the area of a rectangle from width and height."
  [{:keys [width height]}]
  (* width height))

(defmethod shape-area :default
  "Fallback for unknown shape types."
  [shape]
  (throw (ex-info "Unknown shape type" {:type (:type shape)})))

(defmulti serialize-value
  "Serialize a value to the target format."
  (fn [_value format] format))

(defmethod serialize-value :json
  "Serialize a value to JSON string format."
  [value _format]
  (str "{\"value\":" (pr-str value) "}"))

(defmethod serialize-value :edn
  "Serialize a value to EDN string format."
  [value _format]
  (pr-str value))

(defmulti format-output
  "Format a value for display output based on its class type."
  class)

(defmethod format-output java.lang.String
  "Format a string value for display."
  [s]
  (str "\"" s "\""))

(defmethod format-output :default
  "Format any other value using pr-str."
  [x]
  (pr-str x))

;; --- Macros ---

(defmacro with-timing
  "Execute body and return a map with :result and :elapsed-ms keys."
  [label & body]
  `(let [start# (System/currentTimeMillis)
         result# (do ~@body)
         elapsed# (- (System/currentTimeMillis) start#)]
     (when *debug-mode*
       (println ~label "took" elapsed# "ms"))
     {:result result# :elapsed-ms elapsed#}))

(defmacro with-retry
  "Execute body with retry logic up to max-retries times."
  [max-retries & body]
  `(loop [attempt# 1]
     (let [result# (try
                     {:ok (do ~@body)}
                     (catch Exception e# {:error e#}))]
       (if (:ok result#)
         (:ok result#)
         (if (>= attempt# ~max-retries)
           (throw (:error result#))
           (recur (inc attempt#)))))))

(defmacro defcommand
  "Define a named command with metadata and register it."
  [cmd-name description & body]
  `(do
     (defn ~cmd-name ~description ~@body)
     (swap! -registry conj {:name ~(str cmd-name) :doc ~description})))

(defmacro when-debug
  "Execute body only when *debug-mode* is true."
  [& body]
  `(when *debug-mode* ~@body))

;; --- Control flow and call patterns ---

(defn process-pipeline
  "Run data through a multi-stage processing pipeline.
   Uses threading macros, let bindings, and conditionals."
  [raw-data]
  (let [normalized (-> raw-data
                       (update :name str/trim)
                       (update :name str/lower-case)
                       (assoc :processed-at (Date.)))
        tags (->> (:tags raw-data)
                  (filter string?)
                  (map str/lower-case)
                  (distinct)
                  (into []))]
    (cond
      (nil? (:name normalized))
      (throw (ex-info "Name is required" {:data raw-data}))

      (empty? tags)
      (assoc normalized :tags ["untagged"])

      :else
      (assoc normalized :tags tags))))

(defn find-first-match
  "Find the first item matching the predicate using loop/recur."
  [pred coll]
  (loop [idx 0
         remaining coll]
    (cond
      (empty? remaining) nil
      (pred (first remaining)) {:index idx :value (first remaining)}
      :else (recur (inc idx) (rest remaining)))))

(defn safe-divide
  "Divide two numbers with error handling via try/catch/finally."
  [numerator denominator]
  (try
    (when (zero? denominator)
      (throw (ArithmeticException. "Division by zero")))
    (/ numerator denominator)
    (catch ArithmeticException e
      {:error (.getMessage e)
       :numerator numerator
       :denominator denominator})
    (finally
      (when-debug
        (println "Division attempted:" numerator "/" denominator)))))

;; --- Java interop ---

(defn create-unique-id
  "Generate a unique string identifier using java.util.UUID."
  []
  (str (UUID/randomUUID)))

(defn file-info
  "Gather file metadata as a map using Java File interop."
  [path]
  (let [f (File. ^String path)]
    {:exists (.exists f)
     :size (.length f)
     :name (.getName f)
     :directory (.isDirectory f)}))

;; --- Destructuring ---

(defn process-user
  "Process a user record with map destructuring and defaults."
  [{:keys [name email role]
    :or {role :viewer}
    :as user}]
  {:display-name (str name " <" email ">")
   :permissions (case role
                  :admin #{:read :write :delete :admin}
                  :editor #{:read :write}
                  :viewer #{:read})
   :raw user})

;; --- State management ---

(def ^:private event-log
  "Internal event log for tracking state changes."
  (atom []))

(defn log-event
  "Append an event to the internal event log with a timestamp."
  [event-type data]
  (swap! event-log conj
         {:type event-type
          :data data
          :timestamp (System/currentTimeMillis)}))

(defn get-events
  "Retrieve events from the log, optionally filtered by type."
  ([]
   @event-log)
  ([event-type]
   (filterv #(= event-type (:type %)) @event-log)))

;; --- Metadata annotations ---

(def ^:const ^:private MAX_CACHE_SIZE
  "Maximum entries allowed in the LRU cache."
  10000)

(defn ^:deprecated legacy-process
  "Process data using the legacy algorithm. Use process-batch instead."
  [data]
  (map identity data))

(defn ^{:added "0.5.0" :author "core-team"} transform-record
  "Transform a record using the standard normalization pipeline."
  [record]
  (-> record
      normalize-record
      (assoc :version version)))

;; --- Entry point ---

(defn -main
  "Application entry point. Parses args and runs the processing pipeline."
  [& args]
  (binding [*debug-mode* (some #{"--debug"} args)]
    (let [config (merge-configs default-config
                                (when (first args)
                                  {:input (first args)}))]
      (println "Starting with config:" config)
      (with-timing "main"
        (-> {:name "  Test Record  "
             :tags ["Alpha" "beta" "ALPHA"]}
            process-pipeline
            (assoc :config config))))))

;; --- Reader literals and special forms for grammar coverage ---

;; regex_lit: Regular expression literals
(def email-pattern #"[\w.]+@[\w.]+\.\w+")
(def url-pattern #"https?://[\w./]+")

;; char_lit: Character literals
(def newline-char \newline)
(def space-char \space)
(def letter-a \a)

;; quoting_lit: Quote form
(def quoted-sym 'my-symbol)
(def quoted-list '(1 2 3))

;; var_quoting_lit: Var reference
(def process-var #'process-data)

;; dis_expr: Discard reader macro
(defn with-discard
  "Function with discarded form"
  [x]
  #_(println "debug output")
  (* x 2))

;; tagged_or_ctor_lit: Tagged literals
(def example-date #inst "2026-01-15T00:00:00.000Z")
(def example-uuid #uuid "550e8400-e29b-41d4-a716-446655440000")

;; ns_map_lit: Namespace map literal
(def ns-config #:db{:host "localhost" :port 5432 :name "mydb"})

;; auto_res_mark: Auto-resolved namespace map literal (uses current ns)
(def auto-ns-config #::{:host "localhost" :port 5432})

;; sym_val_lit: Symbolic value literals
(def pos-infinity ##Inf)
(def neg-infinity ##-Inf)
(def not-a-number ##NaN)

;; auto_res_mark: Auto-resolved keywords
(def auto-kw ::local-keyword)
(def auto-ns-kw ::other/namespaced)

;; kwd_ns: Namespaced keywords (explicit namespace)
(def db-host :db/host)
(def user-name :user/name)

;; read_cond_lit: Reader conditionals
(def platform-value
  #?(:clj  "JVM Clojure"
     :cljs "ClojureScript"
     :default "Unknown"))

;; splicing_read_cond_lit: Splicing reader conditionals
(defn platform-imports
  "Returns platform-specific imports"
  []
  [#?@(:clj  [java.util.Date java.util.UUID]
       :cljs [goog.date.Date])])

;; old_meta_lit: Legacy metadata syntax using #^ (pre-1.2, replaced by ^)
(def #^{:deprecated "Use new-fn instead"} legacy-fn identity)

;; evaling_lit: Eval reader macro (rarely used)
;; Note: #= is disabled by default in modern Clojure for security
;; Using a comment reference instead of live code:
;; (def eval-example #=(+ 1 2))
