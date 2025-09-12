import { useAtom } from "jotai";
import { atomWithStorage } from "jotai/utils";

import { Theme } from "@/registry/themes";

export type HSLColor = {
  h: number;
  s: number;
  l: number;
};

export type Config = {
  theme: Theme["name"];
  radius: number;
  saturationRange: Array<number>;
  lightnessRange: Array<number>;
  colors: {
    light: { [key: string]: HSLColor };
    dark: { [key: string]: HSLColor };
  };
};

export const configAtom = atomWithStorage<Config>("config", {
  theme: "violet",
  radius: 0.5,
  saturationRange: [94.09],
  lightnessRange: [39.8],
  colors: {
    light: {
      foreground: { h: 0, s: 0, l: 0 },
      background: { h: 0, s: 0, l: 100 },
      primary: { h: 259.16, s: 94.09, l: 39.8 },
      primaryForeground: { h: 255, s: 100, l: 100 },
      secondary: { h: 0, s: 0, l: 70.98 },
      secondaryForeground: { h: 222.2, s: 47.4, l: 11.2 },
      border: { h: 0, s: 0, l: 83.14 },
      input: { h: 0, s: 0, l: 83.14 },
      card: { h: 0, s: 0, l: 100 },
      cardForeground: { h: 0, s: 0, l: 0 },
      muted: { h: 210, s: 40, l: 96.1 },
      mutedForeground: { h: 0, s: 0, l: 25.88 },
      accent: { h: 210, s: 40, l: 96.1 },
      accentForeground: { h: 222.2, s: 47.4, l: 11.2 },
      ring: { h: 222.2, s: 84, l: 4.9 },
      popover: { h: 0, s: 0, l: 100 },
      popoverForeground: { h: 222.2, s: 84, l: 4.9 },
      destructive: { h: 0, s: 84.2, l: 60.2 },
      destructiveForeground: { h: 210, s: 40, l: 98 },
    },
    dark: {
      foreground: { h: 210, s: 20, l: 98 },
      background: { h: 224, s: 71.4, l: 4.1 },
      primary: { h: 263.4, s: 70, l: 50.4 },
      primaryForeground: { h: 210, s: 20, l: 98 },
      secondary: { h: 215, s: 27.9, l: 16.9 },
      secondaryForeground: { h: 210, s: 20, l: 98 },
      border: { h: 215, s: 27.9, l: 16.9 },
      input: { h: 215, s: 27.9, l: 16.9 },
      card: { h: 224, s: 71.4, l: 4.1 },
      cardForeground: { h: 210, s: 20, l: 98 },
      muted: { h: 215, s: 27.9, l: 16.9 },
      mutedForeground: { h: 217.9, s: 10.6, l: 64.9 },
      accent: { h: 215, s: 27.9, l: 16.9 },
      accentForeground: { h: 210, s: 20, l: 98 },
      ring: { h: 263.4, s: 70, l: 50.4 },
      popover: { h: 224, s: 71.4, l: 4.1 },
      popoverForeground: { h: 210, s: 20, l: 98 },
      destructive: { h: 0, s: 62.8, l: 30.6 },
      destructiveForeground: { h: 210, s: 20, l: 98 },
    },
  },
});

export function useConfig() {
  return useAtom(configAtom);
}
