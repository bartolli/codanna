import Link from 'next/link';
import { getCurrentUser } from '@/lib/auth';

export async function Header() {
  const user = await getCurrentUser();

  return (
    <header className="bg-white border-b">
      <nav className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
        <div className="flex justify-between h-16">
          <div className="flex items-center">
            <Link href="/" className="text-xl font-bold text-gray-900">
              NextApp
            </Link>
            <div className="hidden md:flex ml-10 space-x-8">
              <NavLink href="/">Home</NavLink>
              <NavLink href="/posts">Posts</NavLink>
              {user && <NavLink href="/dashboard">Dashboard</NavLink>}
            </div>
          </div>

          <div className="flex items-center">
            {user ? (
              <UserMenu user={user} />
            ) : (
              <div className="space-x-4">
                <Link
                  href="/login"
                  className="text-gray-600 hover:text-gray-900"
                >
                  Sign in
                </Link>
                <Link
                  href="/register"
                  className="bg-blue-600 text-white px-4 py-2 rounded-md hover:bg-blue-700"
                >
                  Sign up
                </Link>
              </div>
            )}
          </div>
        </div>
      </nav>
    </header>
  );
}

function NavLink({ href, children }) {
  return (
    <Link
      href={href}
      className="text-gray-600 hover:text-gray-900 px-3 py-2 text-sm font-medium"
    >
      {children}
    </Link>
  );
}

function UserMenu({ user }) {
  return (
    <div className="flex items-center space-x-4">
      <span className="text-sm text-gray-700">
        {user.name}
      </span>
      <form action="/api/auth/logout" method="POST">
        <button
          type="submit"
          className="text-sm text-gray-600 hover:text-gray-900"
        >
          Sign out
        </button>
      </form>
    </div>
  );
}

export default Header;
