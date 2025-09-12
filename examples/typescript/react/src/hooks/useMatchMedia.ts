import { useEffect, useState } from "react";

export default function useMediaQuery(query: string) {
  const [matches, setMatches] = useState(false);

  useEffect(() => {
    const media = window.matchMedia(query);

    // Immediate check to set the initial state
    setMatches(media.matches);

    const listener = () => {
      setMatches(media.matches);
    };

    media.addEventListener("change", listener);

    return () => {
      media.removeEventListener("change", listener);
    };
  }, [query]); // Only re-run the effect if query changes

  return matches;
}
