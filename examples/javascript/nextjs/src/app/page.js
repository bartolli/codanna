import Link from 'next/link';
import { findAllPosts } from '@/lib/db';
import { PostCard } from '@/components/PostCard';

export default async function HomePage() {
  const posts = await findAllPosts({ limit: 3 });

  return (
    <div className="space-y-12">
      <section className="text-center py-12">
        <h1 className="text-4xl font-bold text-gray-900 mb-4">
          Welcome to NextApp
        </h1>
        <p className="text-xl text-gray-600 max-w-2xl mx-auto">
          A Next.js example application demonstrating path aliases with jsconfig.json
        </p>
      </section>

      <section>
        <div className="flex items-center justify-between mb-6">
          <h2 className="text-2xl font-semibold text-gray-900">Recent Posts</h2>
          <Link
            href="/posts"
            className="text-blue-600 hover:text-blue-700 font-medium"
          >
            View all
          </Link>
        </div>

        <div className="grid gap-6 md:grid-cols-2 lg:grid-cols-3">
          {posts.map((post) => (
            <PostCard key={post.id} post={post} />
          ))}
        </div>

        {posts.length === 0 && (
          <p className="text-center text-gray-500 py-12">
            No posts yet. Be the first to create one!
          </p>
        )}
      </section>
    </div>
  );
}
