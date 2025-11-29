import { Suspense } from 'react';
import { findAllPosts } from '@/lib/db';
import { PostCard, PostCardSkeleton } from '@/components/PostCard';
import { parseSearchParams } from '@/lib/utils';

export const metadata = {
  title: 'Posts | NextApp',
  description: 'Browse all posts',
};

async function PostList({ page = 1 }) {
  const limit = 10;
  const offset = (page - 1) * limit;
  const posts = await findAllPosts({ limit, offset });

  return (
    <div className="grid gap-6 md:grid-cols-2">
      {posts.map((post) => (
        <PostCard key={post.id} post={post} />
      ))}
    </div>
  );
}

function PostListSkeleton() {
  return (
    <div className="grid gap-6 md:grid-cols-2">
      {Array.from({ length: 6 }).map((_, i) => (
        <PostCardSkeleton key={i} />
      ))}
    </div>
  );
}

export default function PostsPage({ searchParams }) {
  const { page = '1' } = parseSearchParams(new URLSearchParams(searchParams));
  const currentPage = parseInt(page, 10) || 1;

  return (
    <div>
      <h1 className="text-3xl font-bold text-gray-900 mb-8">All Posts</h1>

      <Suspense fallback={<PostListSkeleton />}>
        <PostList page={currentPage} />
      </Suspense>

      <Pagination currentPage={currentPage} />
    </div>
  );
}

function Pagination({ currentPage }) {
  return (
    <nav className="flex justify-center mt-8 space-x-2">
      {currentPage > 1 && (
        <a
          href={`/posts?page=${currentPage - 1}`}
          className="px-4 py-2 border rounded-md hover:bg-gray-50"
        >
          Previous
        </a>
      )}
      <span className="px-4 py-2 bg-blue-600 text-white rounded-md">
        {currentPage}
      </span>
      <a
        href={`/posts?page=${currentPage + 1}`}
        className="px-4 py-2 border rounded-md hover:bg-gray-50"
      >
        Next
      </a>
    </nav>
  );
}
