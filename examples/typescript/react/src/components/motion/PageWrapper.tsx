"use client";

import { motion } from "framer-motion";
import { FC, PropsWithChildren } from "react";
import { cn } from "@/lib/utils";

interface PageWrapperProps {
  className?: string;
}

const PageWrapper: FC<PropsWithChildren<PageWrapperProps>> = ({
  children,
  className,
}) => {
  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: 20 }}
      className={cn("col-start-2", className)}
    >
      {children}
    </motion.div>
  );
};

export default PageWrapper;
