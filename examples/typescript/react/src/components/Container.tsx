import React from "react";
import clsx from "clsx";

/**
 * Container component for creating a responsive grid layout with safe areas.
 * 
 * @description
 * This component creates a responsive grid container that respects safe areas
 * on the left and right sides. It's designed to provide consistent spacing
 * and alignment across different screen sizes in the shadcn theme generator.
 * 
 * @example
 * ```tsx
 * <Container className="my-custom-class">
 *   <Container.Inner>
 *     <h1>Content goes here</h1>
 *   </Container.Inner>
 * </Container>
 * ```
 * 
 * @param {React.ReactNode} children - The content to be rendered inside the container
 * @param {string} [className] - Optional CSS classes to apply to the container
 * @param {React.ElementType} [component="div"] - The HTML element or component to render as the container
 * @returns {React.ReactElement} The rendered container component
 */
const Container: React.FC<
  React.PropsWithChildren<{
    children: React.ReactNode;
    className?: string;

    component?: React.ElementType;
  }>
> & { Inner: typeof Inner } = ({
  children,
  className,
  component: Component = "div",
}) => {
  return (
    <Component
      className={clsx(
        "grid grid-cols-[1fr_var(--safe-area-left)_minmax(0px,1230px)_var(--safe-area-right)_1fr] gap-y-8",
        className,
      )}
    >
      {children}
    </Component>
  );
};

/**
 * Inner component for Container - provides content alignment within the grid.
 * 
 * @description
 * This component is used as a child of Container to properly align content
 * within the grid system. It positions content in the third column of the
 * parent Container's grid, which is the main content area between safe zones.
 * 
 * @example
 * ```tsx
 * <Container>
 *   <Container.Inner className="my-content">
 *     <p>This content is properly aligned</p>
 *   </Container.Inner>
 * </Container>
 * ```
 * 
 * @param {React.ReactNode} children - The content to render inside the inner container
 * @param {string} [className] - Optional CSS classes to apply to the inner container
 * @returns {React.ReactElement} The rendered inner container element
 */
const Inner: React.FC<React.PropsWithChildren<{ className?: string }>> = ({
  children,
  className,
}) => {
  return <div className={clsx("col-start-3", className)}>{children}</div>;
};

Container.Inner = Inner;

export default Container;
