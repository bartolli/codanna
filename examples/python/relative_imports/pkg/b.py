from pkg.a import helper


def absolute_caller(x):
    return helper(x)


def lazy_caller(x):
    from pkg.a import helper

    return helper(x)
