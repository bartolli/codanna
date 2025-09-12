import Image from "next/image";
import Link from "next/link";

import { Announcement } from "@/components/announcement";
import { ExamplesNav } from "@/components/examples-nav";
import {
  PageActions,
  PageHeader,
  PageHeaderDescription,
  PageHeaderHeading,
} from "@/components/page-header";
import { Button } from "@/components/ui/button";
import MailPage from "@/app/(app)/examples/mail/page";
import { ThemeCustomizer } from "@/components/theming/ThemeCustomizer";
import { Card, CardContent, CardHeader } from "@/registry/default/ui/card";
import PageWrapper from "@/components/motion/PageWrapper";
import { ThemeSwitcher } from "@/components/theming/ThemeSwitcher";

export default function IndexPage() {
  return (
    <PageWrapper>
      <Card className="fixed bottom-4 right-4 z-10">
        <CardHeader>Customize</CardHeader>
        <CardContent>
          <ThemeCustomizer />
        </CardContent>
      </Card>
      <div className="relative">
        <PageHeader>
          <Announcement />
          <PageHeaderHeading>Build your component library</PageHeaderHeading>
          <PageHeaderDescription>
            Beautifully designed components that you can copy and paste into
            your apps.
          </PageHeaderDescription>
          <PageActions>
            <Button asChild size="sm">
              <Link href="/docs">Get Started</Link>
            </Button>
            <Button asChild size="sm" variant="ghost">
              <Link target="_blank" rel="noreferrer" href="https://github.com">
                GitHub
              </Link>
            </Button>
          </PageActions>
        </PageHeader>
        <ExamplesNav className="[&>a:first-child]:text-primary" />
        <section className="overflow-hidden rounded-lg border bg-background shadow-md md:hidden md:shadow-xl">
          <Image
            src="/examples/mail-dark.png"
            width={1280}
            height={727}
            alt="Mail"
            className="hidden dark:block"
          />
          <Image
            src="/examples/mail-light.png"
            width={1280}
            height={727}
            alt="Mail"
            className="block dark:hidden"
          />
        </section>
        <section className="hidden md:block">
          <div className="overflow-hidden rounded-lg border bg-background shadow">
            <MailPage />
          </div>
        </section>
      </div>
    </PageWrapper>
  );
}
