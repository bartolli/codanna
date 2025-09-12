import { STRIPE_LINK } from "@/utils/constants";

const links = [
  { href: "/demo", label: "Demo" },
  { href: "https://zippystarter.beehiiv.com/subscribe", label: "Newsletter" },
  {
    href: STRIPE_LINK,
    label: "Buy now",
    button: "default",
  },
];

export { links };
