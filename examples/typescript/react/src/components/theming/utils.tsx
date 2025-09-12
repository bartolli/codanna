import template from "lodash.template";
import { Config, HSLColor } from "@/hooks/use-config";

export const getHslValue = (color: HSLColor) =>
  `${color.h} ${color.s}% ${color.l}%`;

export function getThemeValues(colors: any, mode: "light" | "dark") {
  const tm = colors[mode];

  return {
    foreground: `${tm.foreground.h} ${tm.foreground.s}% ${tm.foreground.l}%`,
    background: `${tm.background.h} ${tm.background.s}% ${tm.background.l}%`,
    primary: `${tm.primary.h} ${tm.primary.s}% ${tm.primary.l}%`,
    primaryForeground: getForegroundHslColor(tm.primary.l),
    secondary: `${tm.secondary.h} ${tm.secondary.s}% ${tm.secondary.l}%`,
    secondaryForeground: getForegroundHslColor(tm.secondary.l),
    border: `${tm.border.h} ${tm.border.s}% ${tm.border.l}%`,
    input: `${tm.input.h} ${tm.input.s}% ${tm.input.l}%`,
    card: `${tm.card.h} ${tm.card.s}% ${tm.card.l}%`,
    cardForeground: `${tm.cardForeground.h} ${tm.cardForeground.s}% ${tm.cardForeground.l}%`,
    muted: `${tm.muted.h} ${tm.muted.s}% ${tm.muted.l}%`,
    mutedForeground: `${tm.mutedForeground.h} ${tm.mutedForeground.s}% ${tm.mutedForeground.l}%`,
    accent: `${tm.accent.h} ${tm.accent.s}% ${tm.accent.l}%`,
    accentForeground: `${tm.accentForeground.h} ${tm.accentForeground.s}% ${tm.accentForeground.l}%`,
    ring: `${tm.ring.h} ${tm.ring.s}% ${tm.ring.l}%`,
    popover: `${tm.popover.h} ${tm.popover.s}% ${tm.popover.l}%`,
    popoverForeground: `${tm.popoverForeground.h} ${tm.popoverForeground.s}% ${tm.popoverForeground.l}%`,
    destructive: `${tm.destructive.h} ${tm.destructive.s}% ${tm.destructive.l}%`,
    destructiveForeground: `${tm.destructiveForeground.h} ${tm.destructiveForeground.s}% ${tm.destructiveForeground.l}%`,
  };
}

export function getThemeCode(theme: Config, isCopy = false) {
  if (!theme) {
    return;
  }

  const lightThemeValues = getThemeValues(theme.colors, "light");
  const darkThemeValues = getThemeValues(theme.colors, "dark");

  const defaultProps = {
    colors: {
      light: lightThemeValues,
      dark: darkThemeValues,
    },
    radius: theme.radius,
  };

  console.log(defaultProps.colors.dark);

  const styleContent = template(BASE_STYLES_WITH_VARIABLES())({
    ...defaultProps,
  });

  const copyContent = template(BASE_STYLES_WITH_VARIABLES(isCopy))({
    ...defaultProps,
  });

  let styleTag = document.getElementById("dynamic-theme-style");

  if (!styleTag) {
    styleTag = document.createElement("style");
    styleTag.id = "dynamic-theme-style";
    document.head.appendChild(styleTag);
  }

  styleTag.innerHTML = styleContent;

  if (isCopy) {
    return copyContent;
  }

  return copyContent;
}

export function getForegroundHslColor(lightness: number) {
  // Using 50% as the threshold for simplicity
  return lightness > 60 ? "0 0% 0%" : "0 0% 100%"; // Black or White
}

// Updated clamp function to accept a range array
const clamp = (value: number, range: number[]) =>
  Math.min(Math.max(value, range[0]), range[1]);

// Updated adjustHSL function
export const adjustHSL = (
  color: HSLColor,
  saturation: Array<number>,
  lightness: Array<number>,
  sRange = [0, 100],
  lRange = [0, 100],
) => {
  return {
    h: color.h,
    s: clamp(saturation[0], sRange),
    l: clamp(lightness[0], lRange),
  };
};

export const hexRegex = /^#([\da-f]{3}){1,2}$/i;

export const defaultColor = { h: 262.1, s: 88.3, l: 57.8 };
export const defaultSaturationRange = [100];
export const defaultLightnessRange = [0];

export const BASE_STYLES_WITH_VARIABLES = (isCopy?: boolean) => `
:root ${isCopy ? "" : ".theme-custom"} {
  --background: <%- colors.light.background %>;
  --foreground: <%- colors.light.foreground %>;
  --card: <%- colors.light.card %>;
  --card-foreground: <%- colors.light.cardForeground %>;
  --popover: <%- colors.light.popover %>;
  --popover-foreground: <%- colors.light.popoverForeground %>;
  --primary: <%- colors.light.primary %>;
  --primary-foreground: <%- colors.light.primaryForeground %>;
  --secondary: <%- colors.light.secondary %>;
  --secondary-foreground: <%- colors.light.secondaryForeground %>;
  --muted: <%- colors.light.muted %>;
  --muted-foreground: <%- colors.light.mutedForeground %>;
  --accent: <%- colors.light.accent %>;
  --accent-foreground: <%- colors.light.accentForeground %>;
  --destructive: <%- colors.light.destructive %>;
  --destructive-foreground: <%- colors.light.destructiveForeground %>;
  --border: <%- colors.light.border %>;
  --input: <%- colors.light.input %>;
  --ring: <%- colors.light.ring %>;
  --radius: <%- radius %>rem;
}
.dark ${isCopy ? "" : ".theme-custom"} {
  --background: <%- colors.dark.background %>;
  --foreground: <%- colors.dark.foreground %>;
  --card: <%- colors.dark.card %>;
  --card-foreground: <%- colors.dark.cardForeground %>;
  --popover: <%- colors.dark.popover %>;
  --popover-foreground: <%- colors.dark.popoverForeground %>;
  --primary: <%- colors.dark.primary %>;
  --primary-foreground: <%- colors.dark.primaryForeground %>;
  --secondary: <%- colors.dark.secondary %>;
  --secondary-foreground: <%- colors.dark.secondaryForeground %>;
  --muted: <%- colors.dark.muted %>;
  --muted-foreground: <%- colors.dark.mutedForeground %>;
  --accent: <%- colors.dark.accent %>;
  --accent-foreground: <%- colors.dark.accentForeground %>;
  --destructive: <%- colors.dark.destructive %>;
  --destructive-foreground: <%- colors.dark.destructiveForeground %>;
  --border: <%- colors.dark.border %>;
  --input: <%- colors.dark.input %>;
  --ring: <%- colors.dark.ring %>;
  --radius: <%- radius %>rem;
}
`;
