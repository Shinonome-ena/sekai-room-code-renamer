/// 空列表 = 所有人可执行管理指令
pub fn is_superuser(user_id: i64, admin_users: &[i64]) -> bool {
    admin_users.is_empty() || admin_users.contains(&user_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_list_allows_all() {
        assert!(is_superuser(123, &[]));
    }

    #[test]
    fn matching_user() {
        assert!(is_superuser(123, &[123, 456]));
    }

    #[test]
    fn non_matching_user() {
        assert!(!is_superuser(789, &[123, 456]));
    }
}
