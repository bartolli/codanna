import CardsDemo from "@/components/registry/default/example/cards";
import { ThemeCustomizer } from "@/components/theming/ThemeCustomizer";
import { TooltipProvider } from "@/components/ui/tooltip";
import { Toaster } from "@/components/ui/toaster";
import { ThemeWrapper } from "@/components/theming/ThemeWrapper";
import PageWrapper from "@/components/motion/PageWrapper";

export default function Home() {
  return (
    <PageWrapper>
      <ThemeWrapper
        defaultTheme="custom"
        className="grid grid-cols-12 gap-y-[clamp(1.5rem,8cqw,2rem)]"
      >
        <Toaster />
        <div className="col-span-12 grid lg:grid-cols-2 z-10 gap-x-6 md:gap-y-9 gap-y-14 items-start text-center lg:text-left">
          <div className="@container grid gap-y-9">
            <h1 className="[text-wrap:balance] text-[clamp(2rem,10cqw,3rem)]/[1.125] font-bold">
              shadcn UI theme generator.
            </h1>
            <p className="font-[300] text-muted-foreground text-fluid-sm/[1.3] text-wrap:balance] -mt-2">
              Easily create custom themes from a single colour that you can copy
              and paste into your apps.
            </p>
          </div>

          <TooltipProvider>
            <ThemeCustomizer />
          </TooltipProvider>
        </div>
        <div className="col-span-12">
          <CardsDemo />
        </div>
      </ThemeWrapper>
    </PageWrapper>
  );
}
