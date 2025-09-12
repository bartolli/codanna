import * as React from "react";
import { CheckIcon, CopyIcon } from "@radix-ui/react-icons";
import { Button } from "@/components/ui/button";
import { useConfig } from "@/hooks/use-config";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/Dialog";
import { ThemeWrapper } from "@/components/theming/ThemeWrapper";
import { copyToClipboardWithMeta } from "@/components/CopyButton";
import { getThemeCode } from "./utils";
import CustomizerCode from "./CustomizerCode";

export default function CopyCodeButton() {
  const [config] = useConfig();
  const [hasCopied, setHasCopied] = React.useState(false);

  React.useEffect(() => {
    setTimeout(() => {
      setHasCopied(false);
    }, 2000);
  }, [hasCopied]);

  return (
    <>
      {config && (
        <Button
          onClick={() => {
            setHasCopied(true);
          }}
          className="md:hidden"
        >
          {hasCopied ? (
            <CheckIcon className="mr-2 h-4 w-4" />
          ) : (
            <CopyIcon className="mr-2 h-4 w-4" />
          )}
          Copy
        </Button>
      )}
      <Dialog>
        <DialogTrigger asChild>
          <Button className="hidden md:flex">Copy code</Button>
        </DialogTrigger>
        <DialogContent className="max-w-2xl outline-none">
          <DialogHeader>
            <DialogTitle>Theme</DialogTitle>
            <DialogDescription>
              Copy and paste the following code into your CSS file.
            </DialogDescription>
          </DialogHeader>
          <ThemeWrapper defaultTheme="zinc" className="relative">
            <CustomizerCode />
            {config && (
              <Button
                size="sm"
                onClick={() => {
                  copyToClipboardWithMeta(
                    // @ts-ignore
                    getThemeCode(config, true),
                  );
                  setHasCopied(true);
                }}
                className="absolute right-4 top-4 bg-muted text-muted-foreground hover:bg-muted hover:text-muted-foreground"
              >
                {hasCopied ? (
                  <CheckIcon className="mr-2 h-4 w-4" />
                ) : (
                  <CopyIcon className="mr-2 h-4 w-4" />
                )}
                Copy
              </Button>
            )}
          </ThemeWrapper>
        </DialogContent>
      </Dialog>
    </>
  );
}
