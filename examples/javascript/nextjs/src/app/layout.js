import { Header } from '@/components/Header';

export const metadata = {
  title: 'Next.js App',
  description: 'Example Next.js application with path aliases',
};

export default function RootLayout({ children }) {
  return (
    <html lang="en">
      <body>
        <Header />
        <main className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
          {children}
        </main>
      </body>
    </html>
  );
}
