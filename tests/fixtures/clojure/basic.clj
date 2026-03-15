(ns test.basic
  (:require [clojure.string :as str]))

(def max-size
  "Maximum allowed size"
  1024)

(defn process
  "Process input data"
  [data]
  (str/trim data))

(defn- internal-helper
  "Private helper function"
  [x]
  (* x 2))

(defprotocol Validator
  "Validation protocol"
  (validate [this input]))

(defrecord StringValidator [pattern]
  Validator
  (validate [this input]
    (re-matches (re-pattern pattern) input)))

(defmacro with-validation
  "Run body with validation"
  [validator input & body]
  `(when (validate ~validator ~input)
     ~@body))
