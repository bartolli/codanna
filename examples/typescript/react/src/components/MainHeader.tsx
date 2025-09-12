"use client";

import Link from "next/link";
import React, { ReactNode } from "react";
import { usePathname } from "next/navigation";
import clsx from "clsx";
import { motion } from "framer-motion";
import { links } from "@/data/links";
import { buttonVariants } from "@/components/ui/button";
import isPathActive from "@/utils/isPathActive";

interface MainHeaderProps {
  href: string;
  logo: ReactNode;
}

const MainHeader: React.FC<MainHeaderProps> = ({ href, logo }) => {
  const pathname = usePathname();

  return (
    <>
      <div className="grid justify-between grid-flow-col pt-8 items-center">
        <Link className="text-2xl font-light text-muted-foreground" href={href}>
          {logo}
        </Link>
        <div className="hidden md:grid grid-flow-col items-center gap-8">
          <nav className="hidden md:grid grid-flow-col gap-8 items-center">
            {links.map(({ href, label, button }, index) => {
              if (button) {
                return (
                  <Link
                    key={href}
                    className={buttonVariants({
                      //@ts-expect-error: SHH
                      variant: button,
                      size: "sm",
                    })}
                    href={href}
                  >
                    {label}
                  </Link>
                );
              }
              return (
                <Link
                  className={clsx(
                    "font-bold relative hover:text-foreground transition-all text-sm",
                    {
                      "text-foreground": isPathActive(pathname, href),
                      "text-muted-foreground": !isPathActive(pathname, href),
                    },
                  )}
                  href={href}
                  key={href}
                >
                  {label}
                  {isPathActive(pathname, href) ? (
                    <motion.span
                      animate={{
                        scale: 1.1,
                      }}
                      transition={{
                        type: "spring",
                        bounce: 0.2,
                        duration: 0.6,
                      }}
                      layoutId="underline"
                      className="absolute left-0 right-0 block h-[0.0625rem] -bottom-2 bg-primary will-change-transform"
                    />
                  ) : null}
                </Link>
              );
            })}
          </nav>
        </div>
      </div>
    </>
  );
};

export default MainHeader;
