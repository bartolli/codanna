import { FC, useEffect, useState } from "react";
import { useDebouncyEffect } from "use-debouncy";
import { HslColorPicker } from "react-colorful";
import * as React from "react";
import hexToHSL from "@/utils/colour/hexToHsl";
import { toast } from "@/components/ui/use-toast";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import { defaultColor, hexRegex } from "@/components/theming/utils";
import { HSLColor } from "@/hooks/use-config";

interface ColorPickerProps {
  initialColor: undefined | HSLColor;
  onChange?: (e: any) => void;
}

const ColorPicker: FC<ColorPickerProps> = ({ onChange, initialColor }) => {
  const [color, setColor] = useState<undefined | HSLColor>(initialColor);
  const [inputValue, setInputValue] = useState("");
  const [isValid, setIsValid] = useState<boolean | undefined>(undefined);

  useDebouncyEffect(() => onChange?.(color), 200, [color]);

  useEffect(() => {
    if (initialColor) {
      setColor(initialColor);
    }
  }, [initialColor]);

  useEffect(() => {
    if (inputValue) {
      if (!hexRegex.test(inputValue)) {
        setIsValid(false);
      } else {
        setIsValid(true);
        setColor(hexToHSL(inputValue));
      }
    }
  }, [inputValue]);

  function onSubmit(e: { preventDefault: () => void }) {
    e.preventDefault();
    if (!isValid) {
      toast({
        description: (
          <pre className="mt-2 w-[340px] rounded-md bg-destructive p-4">
            Invalid HEX color
          </pre>
        ),
      });
    }
  }

  return (
    <form onSubmit={onSubmit} className="grid content-start gap-1.5">
      <Label className="text-xs" htmlFor="color-color">
        Pick a color
      </Label>
      <HslColorPicker
        id="color-color"
        color={color}
        onChange={setColor}
        className="!w-auto md:!w-[200px] mb-2"
      />
      <div className="grid content-start gap-1.5">
        <Label className="text-xs" htmlFor="color-text">
          Or enter a Hex value
        </Label>
        <Input
          id="color-text"
          placeholder="e.g. #84D455"
          value={inputValue}
          onChange={(e) => setInputValue(e.target.value)}
        />
      </div>
    </form>
  );
};

export default ColorPicker;
