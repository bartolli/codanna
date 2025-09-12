"use client";

import * as React from "react";
import { usePathname } from "next/navigation";

export function ThemeSwitcher() {
  const config = {
    theme: "custom",
  };

  const pathname = usePathname();

  React.useEffect(() => {
    return document.body.classList.add(`theme-${config.theme}`);
  }, [pathname, config]);

  return null;
}
