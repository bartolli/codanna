import Link from 'next/link';
import { formatRelativeTime, truncate } from '@/lib/utils';

export function PostCard({ post }) {
  return (
    <article className="bg-white rounded-lg shadow-sm border p-6 hover:shadow-md transition-shadow">
      <Link href={`/posts/${post.id}`}>
        <h2 className="text-xl font-semibold text-gray-900 hover:text-blue-600 mb-2">
          {post.title}
        </h2>
      </Link>

      <p className="text-gray-600 mb-4">
        {truncate(post.content, 150)}
      </p>

      <div className="flex items-center justify-between text-sm text-gray-500">
        <div className="flex items-center space-x-2">
          <AuthorAvatar author={post.author} />
          <span>{post.author?.name || 'Unknown'}</span>
        </div>
        <time dateTime={post.createdAt.toISOString()}>
          {formatRelativeTime(post.createdAt)}
        </time>
      </div>
    </article>
  );
}

function AuthorAvatar({ author }) {
  const initials = author?.name
    ?.split(' ')
    .map((n) => n[0])
    .join('')
    .toUpperCase() || '?';

  return (
    <div className="w-8 h-8 rounded-full bg-gray-200 flex items-center justify-center text-xs font-medium text-gray-600">
      {initials}
    </div>
  );
}

export function PostCardSkeleton() {
  return (
    <article className="bg-white rounded-lg shadow-sm border p-6 animate-pulse">
      <div className="h-6 bg-gray-200 rounded w-3/4 mb-4" />
      <div className="space-y-2 mb-4">
        <div className="h-4 bg-gray-200 rounded w-full" />
        <div className="h-4 bg-gray-200 rounded w-5/6" />
      </div>
      <div className="flex items-center justify-between">
        <div className="flex items-center space-x-2">
          <div className="w-8 h-8 rounded-full bg-gray-200" />
          <div className="h-4 bg-gray-200 rounded w-20" />
        </div>
        <div className="h-4 bg-gray-200 rounded w-24" />
      </div>
    </article>
  );
}

export default PostCard;
