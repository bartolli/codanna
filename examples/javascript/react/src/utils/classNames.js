/**
 * Combines class names, filtering out falsy values
 * Similar to clsx/classnames packages
 */
export function classNames(...classes) {
  return classes
    .flat()
    .filter((cls) => typeof cls === 'string' && cls.length > 0)
    .join(' ');
}

/**
 * Conditionally applies classes based on a condition map
 */
export function conditionalClasses(baseClass, conditions) {
  const classes = [baseClass];

  for (const [className, condition] of Object.entries(conditions)) {
    if (condition) {
      classes.push(className);
    }
  }

  return classNames(...classes);
}

export default classNames;
