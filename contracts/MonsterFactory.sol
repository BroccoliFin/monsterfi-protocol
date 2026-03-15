// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "@openzeppelin/contracts/access/Ownable.sol";

contract MonsterFactory is Ownable {
    address public implementation;
    
    event MonsterLaunched(
        address indexed vault,
        address indexed token,
        address indexed creator,
        string name,
        bytes32 configHash
    );
    
    constructor(address _implementation) Ownable() {
        implementation = _implementation;
    }
    
    function setImplementation(address _newImpl) external onlyOwner {
        implementation = _newImpl;
    }
}
