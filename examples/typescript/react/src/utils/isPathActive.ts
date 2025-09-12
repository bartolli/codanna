const isPathActive = (pathname: string, href: string) => {
  const regex = new RegExp(`^${href}`);
  return regex.test(pathname);
};

export default isPathActive;
