//! Refresh tokens live in the OS vault; the account index contains no secrets.
use mc_core::{CredentialStore, LauncherError, LauncherResult, StoredCredential};

#[derive(Default)]
pub struct SystemCredentialStore;

#[cfg(windows)]
mod platform {
    use super::*;
    use windows_sys::Win32::{Foundation::ERROR_NOT_FOUND, Security::Credentials::*};

    fn target(id: &str) -> Vec<u16> {
        format!("MemoriaLauncher/Microsoft/{id}")
            .encode_utf16()
            .chain(Some(0))
            .collect()
    }

    pub fn load(id: &str) -> LauncherResult<Option<StoredCredential>> {
        let name = target(id);
        let mut entry = std::ptr::null_mut();
        // CredRead owns the allocation until CredFree; copy the blob before freeing it.
        unsafe {
            if CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut entry) == 0 {
                let error = std::io::Error::last_os_error();
                return if error.raw_os_error() == Some(ERROR_NOT_FOUND as i32) {
                    Ok(None)
                } else {
                    Err(error.into())
                };
            }
            let bytes = if (*entry).CredentialBlobSize == 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(
                    (*entry).CredentialBlob,
                    (*entry).CredentialBlobSize as usize,
                )
                .to_vec()
            };
            CredFree(entry.cast());
            let refresh_token = String::from_utf8(bytes).map_err(|_| {
                LauncherError::AuthUnavailable("系统凭据内容无效，请重新登录".into())
            })?;
            Ok(Some(StoredCredential {
                account_id: id.into(),
                refresh_token,
            }))
        }
    }

    pub fn save(value: StoredCredential) -> LauncherResult<()> {
        if value.account_id.is_empty() || value.refresh_token.is_empty() {
            return Err(LauncherError::AuthUnavailable("不能保存空凭据".into()));
        }
        let mut name = target(&value.account_id);
        let mut username: Vec<_> = value.account_id.encode_utf16().chain(Some(0)).collect();
        let mut token = value.refresh_token.into_bytes();
        let entry = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: name.as_mut_ptr(),
            UserName: username.as_mut_ptr(),
            CredentialBlob: token.as_mut_ptr(),
            CredentialBlobSize: token
                .len()
                .try_into()
                .map_err(|_| LauncherError::AuthUnavailable("凭据过长".into()))?,
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            ..Default::default()
        };
        // All pointers refer to the owned buffers above, alive for the duration of the call.
        if unsafe { CredWriteW(&entry, 0) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }

    pub fn delete(id: &str) -> LauncherResult<()> {
        if unsafe { CredDeleteW(target(id).as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_NOT_FOUND as i32) {
                return Err(error.into());
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl CredentialStore for SystemCredentialStore {
    async fn load(&self, account_id: &str) -> LauncherResult<Option<StoredCredential>> {
        #[cfg(windows)]
        {
            platform::load(account_id)
        }
        #[cfg(not(windows))]
        {
            let _ = account_id;
            Err(LauncherError::AuthUnavailable(
                "此平台尚未接入系统凭据存储".into(),
            ))
        }
    }

    async fn save(&self, credential: StoredCredential) -> LauncherResult<()> {
        #[cfg(windows)]
        {
            platform::save(credential)
        }
        #[cfg(not(windows))]
        {
            let _ = credential;
            Err(LauncherError::AuthUnavailable(
                "此平台尚未接入系统凭据存储".into(),
            ))
        }
    }

    async fn delete(&self, account_id: &str) -> LauncherResult<()> {
        #[cfg(windows)]
        {
            platform::delete(account_id)
        }
        #[cfg(not(windows))]
        {
            let _ = account_id;
            Err(LauncherError::AuthUnavailable(
                "此平台尚未接入系统凭据存储".into(),
            ))
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn windows_vault_round_trip_and_logout() {
        let id = format!("test-{}", std::process::id());
        struct Cleanup(String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = platform::delete(&self.0);
            }
        }
        let _cleanup = Cleanup(id.clone());
        assert!(SystemCredentialStore.load(&id).await.unwrap().is_none());
        for token in ["test-refresh-one", "test-refresh-two"] {
            SystemCredentialStore
                .save(StoredCredential {
                    account_id: id.clone(),
                    refresh_token: token.into(),
                })
                .await
                .unwrap();
            assert_eq!(
                SystemCredentialStore
                    .load(&id)
                    .await
                    .unwrap()
                    .unwrap()
                    .refresh_token,
                token
            );
        }
        SystemCredentialStore.delete(&id).await.unwrap();
        assert!(SystemCredentialStore.load(&id).await.unwrap().is_none());
    }
}
