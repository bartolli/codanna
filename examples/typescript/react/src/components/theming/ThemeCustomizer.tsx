"use client";

import * as React from "react";
import { useEffect, useState } from "react";
import { BlendingModeIcon, MoonIcon, SunIcon } from "@radix-ui/react-icons";
import { RESET } from "jotai/utils";
import { useTheme } from "next-themes";
import clsx from "clsx";
import { cn } from "@/lib/utils";
import { HSLColor, useConfig } from "@/hooks/use-config";
import { Button, buttonVariants } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Slider } from "@/components/ui/slider";
import {
  adjustHSL,
  defaultColor,
  defaultSaturationRange,
  defaultLightnessRange,
  getHslValue,
  getThemeCode,
} from "@/components/theming/utils";
import ColorPicker from "@/components/theming/ColorPicker";
import CopyCodeButton from "@/components/theming/CopyCodeButton";
import { Separator } from "@/components/ui/separator";
import {
  Sheet,
  SheetContent,
  SheetFooter,
  SheetTrigger,
} from "@/components/ui/sheet";
import { duo } from "@/data/themes/duo";
import { tailwindColors } from "@/data/themes/tailwind/tailwind-colors";
import { Input } from "@/components/ui/input";
import { ThemeSwitcher } from "@/components/theming/ThemeSwitcher";

export function ThemeCustomizer() {
  const [mounted, setMounted] = React.useState(false);

  React.useEffect(() => {
    setMounted(true);
  }, []);

  return (
    <div className="grid space-x-2">
      <Customizer />
    </div>
  );
}

function Customizer() {
  const [mounted, setMounted] = useState(false);
  const { setTheme: setMode, resolvedTheme: mode } = useTheme();
  const [config, setConfig] = useConfig();
  const [color, setColor] = useState<undefined | HSLColor>(undefined);
  const [initialColor, setInitialColor] = useState<undefined | HSLColor>(
    undefined,
  );
  const [saturationRange, setSaturationRange] = useState([100]);
  const [lightnessRange, setLightnessRange] = useState([0]);

  const getHSL = (s: Array<number>, l: Array<number>, r?: number) => {
    if (color) {
      if (r) {
        const hue = {
          ...color,
          h: color.h + r,
        };
        return adjustHSL(hue, saturationRange, lightnessRange, s, l);
      } else {
        return adjustHSL(color, saturationRange, lightnessRange, s, l);
      }
    }
  };

  const getThemeColors = () => ({
    light: {
      background: getHSL([0, 100], [95, 100]),
      foreground: getHSL([0, 5], [0, 10]),
      primary: color,
      secondary: getHSL([10, 30], [70, 90]),
      border: getHSL([20, 30], [50, 82]),
      input: getHSL([20, 30], [18, 50]),
      card: getHSL([0, 50], [90, 100]),
      cardForeground: getHSL([0, 5], [10, 15]),
      muted: getHSL([10, 30], [85, 95], -38),
      mutedForeground: getHSL([0, 5], [35, 40]),
      accent: getHSL([10, 30], [80, 90], -38),
      accentForeground: getHSL([0, 5], [10, 15]),
      ring: color,
      popover: getHSL([0, 100], [95, 100]),
      popoverForeground: getHSL([95, 100], [0, 10]),
      destructive: adjustHSL(
        { h: 0, s: 100, l: 50 },
        saturationRange,
        lightnessRange,
        [50, 100],
        [30, 50],
      ),
      destructiveForeground: getHSL([0, 5], [90, 100]),
    },
    dark: {
      background: getHSL([10, 50], [5, 10]),
      foreground: getHSL([0, 5], [90, 100]),
      primary: color,
      secondary: getHSL([10, 30], [10, 20]),
      border: getHSL([20, 30], [18, 50]),
      input: getHSL([20, 30], [18, 50]),
      card: getHSL([0, 50], [0, 10]),
      cardForeground: getHSL([0, 5], [90, 100]),
      muted: getHSL([10, 30], [15, 25], -38),
      mutedForeground: getHSL([0, 5], [60, 65]),
      accent: getHSL([10, 30], [15, 25], -38),
      accentForeground: getHSL([0, 5], [90, 95]),
      ring: color,
      popover: getHSL([10, 50], [10, 5]),
      popoverForeground: getHSL([0, 5], [90, 100]),
      destructive: adjustHSL(
        { h: 0, s: 100, l: 50 },
        saturationRange,
        lightnessRange,
        [50, 100],
        [30, 50],
      ),
      destructiveForeground: getHSL([0, 5], [90, 100]),
    },
  });

  const hasColor =
    color?.h !== undefined && color?.s !== undefined && color?.l !== undefined;

  useEffect(() => {
    setMounted(true);
  }, []);

  useEffect(() => {
    const storedTheme = localStorage.getItem("config");

    if (storedTheme) {
      try {
        const ls = JSON.parse(storedTheme);
        const primary = ls.colors?.[mode as string]?.primary;

        setLightnessRange(ls.lightnessRange);
        setSaturationRange(ls.saturationRange);

        if (primary) {
          setInitialColor(primary);
        }
      } catch (error) {
        setInitialColor(defaultColor);
      }
    } else {
      setInitialColor(defaultColor);
    }
  }, [mounted]);

  useEffect(() => {
    if (color && hasColor) {
      const colors = getThemeColors();
      setConfig({
        ...config,
        // @ts-expect-error: Annoying undefined issue
        colors,
        saturationRange,
        lightnessRange,
      });
    }
  }, [color, saturationRange, lightnessRange]);

  useEffect(() => {
    if (color && hasColor) {
      getThemeCode(config);
    }
  }, [config]);

  const handleSaturationChange = (newValue: Array<number>) => {
    setSaturationRange(newValue);
  };

  const handleLightnessChange = (newValue: Array<number>) => {
    setLightnessRange(newValue);
  };

  return (
    <div className="flex flex-col space-y-4 md:space-y-6">
      <ThemeSwitcher />
      <div className="grid md:grid-cols-[auto,1fr] gap-6 text-left">
        <div className="grid content-start gap-1.5">
          <ColorPicker onChange={setColor} initialColor={initialColor} />
        </div>
        <div className="w-full grid gap-y-4">
          <div className="grid gap-y-4">
            <div className="grid content-start gap-1.5">
              <Label
                className="text-xs flex gap-1 items-center"
                htmlFor="range-saturation"
              >
                Saturation
              </Label>
              <Slider
                id="range-saturation"
                value={saturationRange}
                onValueChange={handleSaturationChange}
                max={100}
                step={1}
              />
            </div>

            <div className="grid content-start gap-1.5">
              <Label
                className="text-xs grid content-start gap-1.5 items-center"
                htmlFor="range-lightness"
              >
                Lightness
              </Label>
              <Slider
                id="range-lightness"
                value={lightnessRange}
                onValueChange={handleLightnessChange}
                max={100}
                step={1}
              />
            </div>
          </div>
          <div className="grid gap-y-4">
            <div className="grid content-start gap-y-1.5">
              <Label className="text-xs">Radius</Label>
              <div className="grid grid-cols-5 gap-2">
                {["0", "0.3", "0.5", "0.75", "1.0"].map((value) => {
                  return (
                    <Button
                      variant="outline"
                      size="sm"
                      key={value}
                      onClick={() => {
                        setConfig({
                          ...config,
                          radius: parseFloat(value),
                        });
                      }}
                      className={cn(
                        config.radius === parseFloat(value) &&
                          "border-2 border-primary",
                      )}
                    >
                      {value}
                    </Button>
                  );
                })}
              </div>
            </div>
            <div className="grid content-start gap-y-1.5">
              <Label className="text-xs">Mode</Label>
              <div className="grid grid-cols-3 gap-2">
                {mounted ? (
                  <>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => setMode("light")}
                      className={cn(
                        mode === "light" && "border-2 border-primary",
                      )}
                    >
                      <SunIcon className="mr-1 -translate-x-1" />
                      Light
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => setMode("dark")}
                      className={cn(
                        mode === "dark" && "border-2 border-primary",
                      )}
                    >
                      <MoonIcon className="mr-1 -translate-x-1" />
                      Dark
                    </Button>
                  </>
                ) : (
                  <>
                    <Skeleton className="h-8 w-full" />
                    <Skeleton className="h-8 w-full" />
                  </>
                )}
              </div>
            </div>
          </div>
          <div className="flex flex-wrap gap-4 [&>button]:flex-1">
            {mounted ? (
              <CopyCodeButton />
            ) : (
              <Skeleton className="h-[2.25rem] w-[5.6rem] rounded-full" />
            )}
            <Sheet>
              <SheetTrigger
                className={buttonVariants({ variant: "secondary" })}
              >
                <BlendingModeIcon className="mr-1 h-4 w-4" />
                More
              </SheetTrigger>
              <SheetContent disableBackdrop className="overflow-y-auto">
                <div className="grid gap-6">
                  <div className="grid gap-2">
                    <h4 className="text-sm font-medium leading-none">
                      Ready-made themes
                    </h4>
                    <div className="grid content-start gap-y-1.5">
                      <p className="text-sm text-muted-foreground">Duo-tone</p>
                      <div className="flex flex-wrap gap-1 overflow-x-auto">
                        {duo.map((theme, index) => (
                          <button
                            onClick={() => {
                              //@ts-expect-error: FU
                              setColor(theme.colors?.[mode].primary);
                              //@ts-expect-error: FU
                              setInitialColor(theme.colors?.[mode].primary);
                              handleLightnessChange(theme.lightnessRange);
                              handleSaturationChange(theme.saturationRange);
                            }}
                            key={index}
                            style={{
                              background: `hsl(${getHslValue(
                                theme.colors?.dark.primary,
                              )})`,
                            }}
                            className={clsx("w-6 h-6 rounded-sm")}
                          ></button>
                        ))}
                      </div>
                    </div>
                  </div>
                  <div className="grid gap-2">
                    <p className="text-sm font-medium leading-none">
                      Tailwind colors
                    </p>
                    <div className="grid gap-2">
                      {Object.entries(tailwindColors).map(
                        ([colorName, colorArray], index) => (
                          <div key={index} className="grid">
                            <div className="mb-2">
                              <p className="text-sm text-muted-foreground">
                                {colorName}
                              </p>
                            </div>
                            <div className="grid grid-flow-col gap-1 overflow-x-auto justify-start">
                              {colorArray.map((color, colorIndex) => (
                                <button
                                  key={colorIndex}
                                  style={{
                                    background: `hsl(${getHslValue(
                                      color.primary,
                                    )})`,
                                  }}
                                  className="w-6 h-6 rounded-sm"
                                  onClick={() => {
                                    setColor(color.primary);
                                    setInitialColor(color.primary);
                                  }}
                                ></button>
                              ))}
                            </div>
                          </div>
                        ),
                      )}
                    </div>
                  </div>
                  <Separator />
                  <div className="grid gap-1">
                    <h4 className="text-sm font-medium leading-none">
                      Properties
                    </h4>
                    <div className="grid content-start gap-y-1.5">
                      <p className="text-sm text-muted-foreground">
                        Border radius
                      </p>
                      <div className="flex flex-wrap gap-2">
                        <Input
                          value={config.radius}
                          type="number"
                          step="0.1"
                          min={0}
                          max={2.5}
                          onChange={(e) =>
                            setConfig({
                              ...config,
                              radius: parseFloat(e.target.value),
                            })
                          }
                        />
                      </div>
                    </div>
                  </div>
                </div>
                <SheetFooter className="my-4">
                  <Button
                    variant="secondary"
                    className="w-full"
                    onClick={() => {
                      setConfig(RESET);
                      setInitialColor(defaultColor);
                      handleSaturationChange(defaultSaturationRange);
                      handleLightnessChange(defaultLightnessRange);
                    }}
                  >
                    Reset
                  </Button>
                </SheetFooter>
              </SheetContent>
            </Sheet>
          </div>
        </div>
      </div>
    </div>
  );
}
