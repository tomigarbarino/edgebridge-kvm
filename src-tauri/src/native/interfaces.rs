use std::{collections::BTreeSet, io, mem::size_of, net::Ipv4Addr, ptr::null};
use windows_sys::Win32::{
    Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_NO_DATA, NO_ERROR},
    NetworkManagement::{
        IpHelper::{
            GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST,
            GetAdaptersAddresses, IF_TYPE_ETHERNET_CSMACD, IF_TYPE_IEEE80211, IF_TYPE_PPP,
            IF_TYPE_SOFTWARE_LOOPBACK, IF_TYPE_TUNNEL, IP_ADAPTER_ADDRESSES_LH,
        },
        Ndis::IfOperStatusUp,
    },
    Networking::WinSock::{AF_INET, IpDadStatePreferred, SOCKADDR_IN},
};

pub fn lan_addresses() -> io::Result<BTreeSet<(Ipv4Addr, u8, &'static str)>> {
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut length = 15_000_u32;
    for _ in 0..3 {
        // Windows fills an aligned buffer with linked lists that live in it.
        let mut data = vec![0_u64; (length as usize).div_ceil(size_of::<u64>())];
        let first = data.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
        let result =
            unsafe { GetAdaptersAddresses(u32::from(AF_INET), flags, null(), first, &mut length) };
        match result {
            ERROR_BUFFER_OVERFLOW => continue,
            ERROR_NO_DATA => return Ok(BTreeSet::new()),
            NO_ERROR => {
                let mut addresses = BTreeSet::new();
                let mut next = first;
                // A successful call keeps the pointers valid until data is dropped.
                while let Some(adapter) = unsafe { next.as_ref() } {
                    next = adapter.Next;
                    if adapter.OperStatus != IfOperStatusUp
                        || matches!(
                            adapter.IfType,
                            IF_TYPE_PPP | IF_TYPE_SOFTWARE_LOOPBACK | IF_TYPE_TUNNEL
                        )
                    {
                        continue;
                    }
                    let kind = adapter_kind(adapter);
                    let mut node = adapter.FirstUnicastAddress;
                    while let Some(unicast) = unsafe { node.as_ref() } {
                        node = unicast.Next;
                        if unicast.DadState != IpDadStatePreferred
                            || unicast.Address.iSockaddrLength < size_of::<SOCKADDR_IN>() as i32
                        {
                            continue;
                        }
                        if let Some(address) =
                            unsafe { unicast.Address.lpSockaddr.cast::<SOCKADDR_IN>().as_ref() }
                            && address.sin_family == AF_INET
                        {
                            let octets = unsafe { address.sin_addr.S_un.S_addr.to_ne_bytes() };
                            addresses.insert((
                                Ipv4Addr::from(octets),
                                unicast.OnLinkPrefixLength,
                                kind,
                            ));
                        }
                    }
                }
                return Ok(addresses);
            }
            code => return Err(io::Error::from_raw_os_error(code as i32)),
        }
    }
    Err(io::Error::from_raw_os_error(ERROR_BUFFER_OVERFLOW as i32))
}

fn adapter_kind(adapter: &IP_ADAPTER_ADDRESSES_LH) -> &'static str {
    match adapter.IfType {
        IF_TYPE_IEEE80211 => "Wi-Fi",
        IF_TYPE_ETHERNET_CSMACD => {
            let description = adapter.Description;
            if !description.is_null() {
                // Windows provides a null-terminated description in the adapter buffer.
                let text: Vec<_> = (0..128)
                    .map(|index| unsafe { *description.add(index) })
                    .take_while(|&unit| unit != 0)
                    .collect();
                if String::from_utf16_lossy(&text)
                    .to_ascii_lowercase()
                    .contains("hyper-v")
                {
                    return "Hyper-V";
                }
            }
            "Ethernet"
        }
        _ => "Other LAN",
    }
}
