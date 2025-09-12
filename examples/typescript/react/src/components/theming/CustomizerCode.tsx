import * as React from "react";
import { useConfig } from "@/hooks/use-config";
import { getThemeValues } from "@/components/theming/utils";
import { ThemeWrapper } from "@/components/theming/ThemeWrapper";

export default function CustomizerCode() {
  const [config] = useConfig();

  const lightThemeValues = getThemeValues(config.colors, "light");
  const darkThemeValues = getThemeValues(config.colors, "dark");

  return (
    <ThemeWrapper defaultTheme="zinc" className="relative space-y-4">
      <div data-rehype-pretty-code-fragment="">
        <pre className="max-h-[450px] overflow-x-auto rounded-lg border !py-0 bg-zinc-950 dark:bg-zinc-900">
          <code className="relative rounded bg-muted px-[0.3rem] py-4 font-mono text-sm">
            <span className="line text-foreground">:root &#123;</span>
            <span className="line text-foreground">
              &nbsp;&nbsp;--background: {lightThemeValues?.background};
            </span>
            <span className="line text-foreground">
              &nbsp;&nbsp;--foreground: {lightThemeValues?.foreground};
            </span>
            {[
              "card",
              "popover",
              "primary",
              "secondary",
              "muted",
              "accent",
              "destructive",
            ].map((prefix) => {
              return (
                <React.Fragment key={prefix}>
                  <span className="line text-foreground">
                    &nbsp;&nbsp;--{prefix}:{" "}
                    {
                      lightThemeValues?.[
                        prefix as keyof typeof lightThemeValues
                      ]
                    }
                    ;
                  </span>
                  <span className="line text-foreground">
                    &nbsp;&nbsp;--{prefix}-foreground:{" "}
                    {
                      lightThemeValues?.[
                        `${prefix}Foreground` as keyof typeof lightThemeValues
                      ]
                    }
                    ;
                  </span>
                </React.Fragment>
              );
            })}
            <span className="line text-foreground">
              &nbsp;&nbsp;--border: {lightThemeValues?.border};
            </span>
            <span className="line text-foreground">
              &nbsp;&nbsp;--input: {lightThemeValues?.input};
            </span>
            <span className="line text-foreground">
              &nbsp;&nbsp;--ring: {lightThemeValues?.ring};
            </span>
            <span className="line text-foreground">
              &nbsp;&nbsp;--radius: {config.radius}rem;
            </span>
            <span className="line text-foreground">&nbsp;&#125;</span>
            <span className="line text-foreground">.dark &#123;</span>
            <span className="line text-foreground">
              &nbsp;&nbsp;--background: {darkThemeValues?.background};
            </span>
            <span className="line text-foreground">
              &nbsp;&nbsp;--foreground: {darkThemeValues?.foreground};
            </span>
            {[
              "card",
              "popover",
              "primary",
              "secondary",
              "muted",
              "accent",
              "destructive",
            ].map((prefix) => {
              return (
                <React.Fragment key={prefix}>
                  <span className="line text-foreground">
                    &nbsp;&nbsp;--{prefix}:{" "}
                    {darkThemeValues?.[prefix as keyof typeof darkThemeValues]};
                  </span>
                  <span className="line text-foreground">
                    &nbsp;&nbsp;--{prefix}-foreground:{" "}
                    {
                      darkThemeValues?.[
                        `${prefix}Foreground` as keyof typeof darkThemeValues
                      ]
                    }
                    ;
                  </span>
                </React.Fragment>
              );
            })}
            <span className="line text-foreground">
              &nbsp;&nbsp;--border: {darkThemeValues?.border};
            </span>
            <span className="line text-foreground">
              &nbsp;&nbsp;--input: {darkThemeValues?.input};
            </span>
            <span className="line text-foreground">
              &nbsp;&nbsp;--ring: {darkThemeValues?.ring};
            </span>
            <span className="line text-foreground">
              &nbsp;&nbsp;--radius: {config.radius}rem;
            </span>
            <span className="line text-foreground">&nbsp;&#125;</span>
          </code>
        </pre>
      </div>
    </ThemeWrapper>
  );
}
